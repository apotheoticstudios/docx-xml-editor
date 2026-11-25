use anyhow::{bail, ensure, Context, Result};
use roxmltree::Document;
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_SIZE: u64 = 512 * 1024 * 1024;
pub const CORE: &str = "docProps/core.xml";
pub const APP: &str = "docProps/app.xml";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const CP: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const EP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

#[derive(Clone)]
struct Entry {
    name: String,
    data: Vec<u8>,
    directory: bool,
    method: CompressionMethod,
}
#[derive(Clone)]
pub struct Package {
    pub source: PathBuf,
    entries: Vec<Entry>,
    pub changes: BTreeMap<String, Vec<u8>>,
}
impl Package {
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.metadata()?.len() <= MAX_SIZE,
            "Archive exceeds the 512 MB limit."
        );
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Self::from_bytes(path.to_path_buf(), &data)
    }
    fn from_bytes(source: PathBuf, data: &[u8]) -> Result<Self> {
        let mut archive = ZipArchive::new(Cursor::new(data))
            .context("This file is not a readable ZIP/DOCX archive.")?;
        ensure!(archive.len() <= 10000, "Archive contains too many parts.");
        let mut names = HashSet::new();
        let mut total = 0_u64;
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            let mut part = archive.by_index(i)?;
            let name = part.name().to_owned();
            ensure!(
                part.enclosed_name().is_some()
                    && !name.contains('\\')
                    && !name.contains(':')
                    && !name.split('/').any(|p| p == ".."),
                "Unsafe archive path: {name}"
            );
            ensure!(
                names.insert(name.clone()),
                "Duplicate archive entry: {name}"
            );
            ensure!(
                part.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "Symbolic links are unsupported."
            );
            total = total
                .checked_add(part.size())
                .context("Invalid archive size.")?;
            ensure!(total <= MAX_SIZE, "Expanded archive exceeds 512 MB.");
            let method = part.compression();
            ensure!(
                matches!(
                    method,
                    CompressionMethod::Stored | CompressionMethod::Deflated
                ),
                "Unsupported ZIP compression in {name}."
            );
            let directory = part.is_dir();
            let mut bytes = Vec::new();
            part.read_to_end(&mut bytes)
                .context("Could not read a part or verify its checksum.")?;
            entries.push(Entry {
                name,
                data: bytes,
                directory,
                method,
            });
        }
        let package = Self {
            source,
            entries,
            changes: BTreeMap::new(),
        };
        package.validate()?;
        Ok(package)
    }
    pub fn xml_parts(&self) -> Vec<String> {
        let mut parts: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.name.ends_with(".xml") || e.name.ends_with(".rels"))
            .map(|e| e.name.clone())
            .collect();
        parts.sort();
        parts
    }
    pub fn data(&self, name: &str) -> Result<&[u8]> {
        if let Some(bytes) = self.changes.get(name) {
            return Ok(bytes);
        }
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.data.as_slice())
            .with_context(|| format!("Missing package part: {name}"))
    }
    pub fn text(&self, name: &str) -> Result<&str> {
        std::str::from_utf8(self.data(name)?).with_context(|| {
            format!("{name} is not UTF-8. This editor currently supports UTF-8 XML parts.")
        })
    }
    pub fn set_text(&mut self, name: &str, text: String) -> Result<()> {
        let original = &self
            .entries
            .iter()
            .find(|e| e.name == name)
            .context("Part not found.")?
            .data;
        if original.as_slice() == text.as_bytes() {
            self.changes.remove(name);
        } else {
            self.changes.insert(name.to_owned(), text.into_bytes());
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        for part in ["[Content_Types].xml", "_rels/.rels", "word/document.xml"] {
            self.data(part).with_context(|| format!("Missing {part}. The archive must contain the package contents, not an enclosing folder."))?;
        }
        for part in self.xml_parts() {
            let xml = self.text(&part)?;
            let doc = Document::parse(xml).with_context(|| format!("Invalid XML in {part}"))?;
            for node in doc.descendants().filter(|n| n.is_element()) {
                if let Some(value) = node.attribute((XSI, "type")) {
                    if let Some((prefix, _)) = value.split_once(':') {
                        ensure!(
                            node.lookup_namespace_uri(Some(prefix)).is_some(),
                            "{part}: {value} uses the undeclared namespace {prefix}."
                        );
                    }
                }
                let tag = node.tag_name();
                if (part == APP && tag.name() == "TotalTime")
                    || (part == CORE && tag.name() == "revision")
                {
                    ensure!(
                        node.text().unwrap_or("").parse::<u32>().is_ok(),
                        "{part}: {} must be a nonnegative whole number.",
                        tag.name()
                    );
                }
            }
            if part.ends_with(".rels") {
                let mut ids = HashSet::new();
                for node in doc.root_element().children().filter(|n| n.is_element()) {
                    let id = node.attribute("Id").context("Relationship has no Id.")?;
                    ensure!(
                        !id.is_empty() && ids.insert(id),
                        "Duplicate or empty relationship Id in {part}."
                    );
                    if node.attribute("TargetMode") == Some("External") {
                        continue;
                    }
                    let target = node
                        .attribute("Target")
                        .context("Relationship has no Target.")?;
                    let target = target.split('#').next().unwrap_or("");
                    if target.is_empty() {
                        continue;
                    }
                    let target = percent_decode(target)?;
                    let base = Path::new(&part)
                        .parent()
                        .and_then(Path::parent)
                        .unwrap_or(Path::new(""));
                    let joined = if target.starts_with('/') {
                        PathBuf::from(target.trim_start_matches('/'))
                    } else {
                        base.join(target)
                    };
                    let mut normalized = PathBuf::new();
                    for item in joined.components() {
                        match item {
                            Component::Normal(p) => normalized.push(p),
                            Component::CurDir => {}
                            Component::ParentDir => {
                                ensure!(normalized.pop(), "Relationship escapes the package root.");
                            }
                            _ => bail!("Invalid relationship path in {part}."),
                        }
                    }
                    let resolved = normalized.to_string_lossy().replace('\\', "/");
                    self.data(&resolved)
                        .with_context(|| format!("Broken relationship in {part}: {resolved}"))?;
                }
            }
        }
        Ok(())
    }
    pub fn property(&self, part: &str, name: &str) -> Result<String> {
        let doc = Document::parse(self.text(part)?)?;
        Ok(doc
            .descendants()
            .find(|n| {
                n.is_element()
                    && n.tag_name().name() == name
                    && n.tag_name().namespace() == Some(property_namespace(part, name))
            })
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_owned())
    }
    pub fn set_property(&mut self, part: &str, name: &str, value: &str) -> Result<()> {
        let xml = self.text(part)?;
        let doc = Document::parse(xml)?;
        let ns = property_namespace(part, name);
        let escaped = escape(value);
        let updated = if let Some(node) = doc.descendants().find(|n| {
            n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(ns)
        }) {
            let range = node.range();
            let raw = &xml[range.clone()];
            let end = tag_end(raw)?;
            let replacement = if raw[..=end].trim_end().ends_with("/>") {
                let opening = raw[..end].trim_end().trim_end_matches('/').trim_end();
                let tag = opening
                    .trim_start_matches('<')
                    .split_whitespace()
                    .next()
                    .context("Invalid element.")?;
                format!("{opening}>{escaped}</{tag}>")
            } else {
                let closing = raw
                    .rfind("</")
                    .context("Property must be a text element.")?;
                format!("{}{escaped}{}", &raw[..=end], &raw[closing..])
            };
            let mut result = xml.to_owned();
            result.replace_range(range, &replacement);
            result
        } else {
            let root = doc.root_element();
            let end = xml.rfind("</").context("Missing closing root element.")?;
            let prefix = if ns == DC {
                "dc"
            } else if ns == CP {
                "cp"
            } else {
                "ep"
            };
            let addition =
                format!("<{prefix}:{name} xmlns:{prefix}=\"{ns}\">{escaped}</{prefix}:{name}>");
            ensure!(root.range().end >= end, "Invalid root element.");
            let mut result = xml.to_owned();
            result.insert_str(end, &addition);
            result
        };
        self.set_text(part, updated)
    }
    pub fn save(&self, destination: &Path) -> Result<Option<PathBuf>> {
        self.validate()?;
        ensure!(
            self.changes.is_empty()
                || !self
                    .entries
                    .iter()
                    .any(|e| e.name.starts_with("_xmlsignatures/")),
            "Editing would invalidate this document’s digital signature. Use an unsigned copy."
        );
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        for entry in &self.entries {
            let options = SimpleFileOptions::default().compression_method(entry.method);
            if entry.directory {
                writer.add_directory(&entry.name, options)?;
            } else {
                writer.start_file(&entry.name, options)?;
                writer.write_all(self.data(&entry.name)?)?;
            }
        }
        let bytes = writer.finish()?.into_inner();
        let verified = Self::from_bytes(destination.to_path_buf(), &bytes)?;
        ensure!(
            verified.entries.len() == self.entries.len(),
            "Package part count changed unexpectedly."
        );
        for entry in &self.entries {
            ensure!(
                self.data(&entry.name)? == verified.data(&entry.name)?,
                "Saved data differs in {}.",
                entry.name
            );
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        let backup = if destination.exists() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let name = destination
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let path = parent.join(format!("{name}.backup-{stamp}.docx"));
            std::fs::copy(destination, &path)?;
            Some(path)
        } else {
            None
        };
        temp.persist(destination)
            .map_err(|e| e.error)
            .context("Could not replace the destination. Close it in Word and try again.")?;
        Ok(backup)
    }
}
fn property_namespace(part: &str, key: &str) -> &'static str {
    if part == APP {
        EP
    } else if matches!(key, "title" | "subject" | "creator" | "description") {
        DC
    } else {
        CP
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn tag_end(raw: &str) -> Result<usize> {
    let mut quote = None;
    for (i, c) in raw.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '>' {
            return Ok(i);
        }
    }
    bail!("Unclosed XML tag.")
}
fn percent_decode(value: &str) -> Result<String> {
    let mut output = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure!(i + 2 < bytes.len(), "Invalid escaped relationship target.");
            output.push(u8::from_str_radix(&value[i + 1..i + 3], 16)?);
            i += 3;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8(output)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> Package {
        let parts = [
            ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>"),
            ("_rels/.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"document\" Target=\"word/document.xml\"/></Relationships>"),
            ("word/document.xml", "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body/></w:document>"),
            (CORE, "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title/><cp:revision>1</cp:revision><dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-01-01T00:00:00Z</dcterms:created></cp:coreProperties>"),
            (APP, "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><TotalTime>10</TotalTime></Properties>"),
        ];
        Package {
            source: PathBuf::from("fixture.docx"),
            changes: BTreeMap::new(),
            entries: parts
                .iter()
                .map(|(n, d)| Entry {
                    name: n.to_string(),
                    data: d.as_bytes().to_vec(),
                    directory: false,
                    method: CompressionMethod::Deflated,
                })
                .collect(),
        }
    }
    #[test]
    fn property_edits_preserve_date_namespace_and_escape_text() {
        let mut p = fixture();
        p.set_property(CORE, "title", "Research <copy> & notes")
            .unwrap();
        p.set_property(CORE, "description", "Synthetic research fixture")
            .unwrap();
        p.validate().unwrap();
        assert_eq!(
            p.property(CORE, "title").unwrap(),
            "Research <copy> & notes"
        );
        assert!(p.text(CORE).unwrap().contains("xmlns:dcterms="));
        assert!(p
            .text(CORE)
            .unwrap()
            .contains("xsi:type=\"dcterms:W3CDTF\""));
    }
    #[test]
    fn rejects_undeclared_namespace_in_attribute_value() {
        let mut p = fixture();
        let broken = p
            .text(CORE)
            .unwrap()
            .replace("xsi:type=\"dcterms:W3CDTF\"", "xsi:type=\"missing:W3CDTF\"");
        p.set_text(CORE, broken).unwrap();
        assert!(p
            .validate()
            .unwrap_err()
            .to_string()
            .contains("undeclared namespace"));
    }
    #[test]
    fn rejects_invalid_xml_and_fractional_minutes() {
        let mut p = fixture();
        p.set_text(
            APP,
            "<Properties><TotalTime>5.87</TotalTime></Properties>".into(),
        )
        .unwrap();
        assert!(p.validate().is_err());
        p.set_text(APP, "<Properties><TotalTime>10</Wrong></Properties>".into())
            .unwrap();
        assert!(p.validate().is_err());
    }
    #[test]
    fn round_trip_preserves_all_parts_and_overwrite_creates_backup() {
        let mut p = fixture();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("copy.docx");
        assert!(p.save(&target).unwrap().is_none());
        let original_bytes = std::fs::read(&target).unwrap();
        p.set_property(CORE, "title", "Test title").unwrap();
        let backup = p.save(&target).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), original_bytes);
        let saved = Package::open(&target).unwrap();
        for e in &p.entries {
            assert_eq!(p.data(&e.name).unwrap(), saved.data(&e.name).unwrap());
        }
    }
    #[test]
    fn rejects_broken_relationships_and_unsafe_paths() {
        let mut p = fixture();
        let xml = p
            .text("_rels/.rels")
            .unwrap()
            .replace("word/document.xml", "missing.xml");
        p.set_text("_rels/.rels", xml).unwrap();
        assert!(p.validate().is_err());
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("../escape.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"<a/>").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(Package::from_bytes(PathBuf::from("bad.docx"), &bytes).is_err());
    }
}
