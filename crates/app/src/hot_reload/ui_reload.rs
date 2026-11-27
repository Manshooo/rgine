use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};

use engine_asset::SourceFiles;
use engine_ui::{StyleSheet, WidgetTree};

use super::{POLL_INTERVAL, source_error};

/// The last valid pair of authored UI assets.
pub struct UiDocument {
    /// The authored widget hierarchy.
    pub tree: WidgetTree,
    /// The stylesheet applied to the hierarchy.
    pub styles: StyleSheet,
}

/// Watches the XML layout and CSS stylesheet of a development UI.
pub struct UiReload {
    layout: PathBuf,
    stylesheet: PathBuf,
    sources: SourceFiles,
    last_poll: Instant,
}

impl UiReload {
    /// Parses both files before returning the initial document.
    pub fn load(layout: PathBuf, stylesheet: PathBuf) -> io::Result<(Self, UiDocument)> {
        let mut reload = Self {
            layout,
            stylesheet,
            sources: SourceFiles::default(),
            last_poll: Instant::now(),
        };
        let document = reload.read()?;
        Ok((reload, document))
    }

    /// Publishes a new document when an edit leaves both files valid.
    /// A failed edit is reported once and leaves the active document intact.
    pub fn poll(&mut self, active: &mut UiDocument) -> io::Result<bool> {
        let now = Instant::now();
        if now.duration_since(self.last_poll) < POLL_INTERVAL {
            return Ok(false);
        }
        self.last_poll = now;
        if !self.sources.changed() {
            return Ok(false);
        }
        *active = self.read()?;
        Ok(true)
    }

    fn read(&mut self) -> io::Result<UiDocument> {
        // Record both inputs even when one fails, so changes to either file
        // can recover the document and the failed edit is not reported again.
        let xml = self.sources.read(&self.layout);
        let css = self.sources.read(&self.stylesheet);
        let tree = WidgetTree::parse(&self.layout.to_string_lossy(), text(&self.layout, &xml)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let styles = StyleSheet::parse(
            &self.stylesheet.to_string_lossy(),
            text(&self.stylesheet, &css)?,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(UiDocument { tree, styles })
    }
}

fn text<'a>(path: &Path, content: &'a io::Result<Vec<u8>>) -> io::Result<&'a str> {
    let bytes = content
        .as_deref()
        .map_err(|error| source_error(path, error))?;
    std::str::from_utf8(bytes).map_err(|error| source_error(path, error))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use engine_ui::WidgetKind;

    use super::*;

    const XML: &str = "<ui><panel><label text=\"Old\" /></panel></ui>";
    const CSS: &str = "label { color: white; }";

    fn poll(reload: &mut UiReload, document: &mut UiDocument) -> io::Result<bool> {
        reload.last_poll = Instant::now() - POLL_INTERVAL;
        reload.poll(document)
    }

    fn assert_label(document: &UiDocument, text: &str, color: [u8; 4]) {
        let styled = document
            .styles
            .resolve(&document.tree, |_, _| Default::default());
        let label = &styled.children[0];
        assert_eq!(
            label.widget.kind(),
            &WidgetKind::Label { text: text.into() }
        );
        assert_eq!(label.style.typography.color, color);
    }

    #[test]
    fn xml_and_css_edits_publish_together_only_after_both_parse() {
        let dir = tempfile::tempdir().unwrap();
        let xml = dir.path().join("hud.xml");
        let css = dir.path().join("hud.css");
        fs::write(&xml, XML).unwrap();
        fs::write(&css, CSS).unwrap();
        let (mut reload, mut active) = UiReload::load(xml.clone(), css.clone()).unwrap();
        assert_label(&active, "Old", [255; 4]);

        fs::write(&xml, XML.replace("Old", "New")).unwrap();
        fs::write(&css, "label {\n    nonsense: white;\n}").unwrap();
        let error = poll(&mut reload, &mut active).unwrap_err().to_string();
        assert!(error.contains("hud.css:2:"), "{error}");
        assert_label(&active, "Old", [255; 4]);
        assert!(!poll(&mut reload, &mut active).unwrap());

        fs::write(&css, CSS.replace("white", "black")).unwrap();
        assert!(poll(&mut reload, &mut active).unwrap());
        assert_label(&active, "New", [0, 0, 0, 255]);

        fs::write(&xml, "<ui>\n<panel><wrong /></panel>\n</ui>").unwrap();
        let error = poll(&mut reload, &mut active).unwrap_err().to_string();
        assert!(error.contains("hud.xml:2:"), "{error}");
        assert_label(&active, "New", [0, 0, 0, 255]);
        assert!(!poll(&mut reload, &mut active).unwrap());

        fs::remove_file(&xml).unwrap();
        assert!(poll(&mut reload, &mut active).is_err());
        assert!(!poll(&mut reload, &mut active).unwrap());
        fs::write(&xml, XML).unwrap();
        assert!(poll(&mut reload, &mut active).unwrap());
        assert_label(&active, "Old", [0, 0, 0, 255]);
    }
}
