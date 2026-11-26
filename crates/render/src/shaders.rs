use std::borrow::Cow;

#[cfg(feature = "shader-reload")]
use std::{io, path::Path};

use crate::resources::BindingMode;

const VARIANT_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum ShaderVariantKey {
    Primitive(BindingMode),
    DirectionalShadow,
    Skybox,
}

pub(crate) const SHADER_VARIANTS: &[ShaderVariantKey] = &[
    ShaderVariantKey::Primitive(BindingMode::Classic),
    ShaderVariantKey::Primitive(BindingMode::Bindless),
    ShaderVariantKey::DirectionalShadow,
    ShaderVariantKey::Skybox,
];

const _: () = enforce_budget(SHADER_VARIANTS.len());

const fn enforce_budget(count: usize) {
    assert!(count <= VARIANT_LIMIT, "shader variant budget exceeded");
}

#[derive(Clone, Copy)]
enum ShaderFile {
    Object,
    Primitive,
    Classic,
    Bindless,
    Shadow,
    Skybox,
}

const FILES: [(ShaderFile, &str, &str); 6] = [
    (
        ShaderFile::Object,
        "object.wgsl",
        include_str!("scene/object.wgsl"),
    ),
    (
        ShaderFile::Primitive,
        "primitive.wgsl",
        include_str!("scene/primitive.wgsl"),
    ),
    (
        ShaderFile::Classic,
        "primitive_classic.wgsl",
        include_str!("scene/primitive_classic.wgsl"),
    ),
    (
        ShaderFile::Bindless,
        "primitive_bindless.wgsl",
        include_str!("scene/primitive_bindless.wgsl"),
    ),
    (
        ShaderFile::Shadow,
        "shadow.wgsl",
        include_str!("scene/shadow.wgsl"),
    ),
    (
        ShaderFile::Skybox,
        "skybox.wgsl",
        include_str!("scene/skybox.wgsl"),
    ),
];

/// The WGSL inputs for the registered scene pipeline variants.
#[derive(Clone, PartialEq, Eq)]
pub struct SceneShaderSources {
    files: [Cow<'static, str>; FILES.len()],
}

impl Default for SceneShaderSources {
    fn default() -> Self {
        Self {
            files: FILES.map(|(_, _, source)| Cow::Borrowed(source)),
        }
    }
}

impl SceneShaderSources {
    /// Reads a development shader directory and records the source files it accesses.
    #[cfg(feature = "shader-reload")]
    pub fn load(directory: &Path, tracked: &mut engine_asset::SourceFiles) -> io::Result<Self> {
        let mut sources = Self::default();
        for (file, name, _) in FILES {
            let path = directory.join(name);
            let bytes = tracked.read(&path).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            let text = String::from_utf8(bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {error}", path.display()),
                )
            })?;
            sources.files[file as usize] = Cow::Owned(text);
        }
        Ok(sources)
    }

    #[cfg(feature = "shader-reload")]
    pub(crate) fn changed_files<'a>(
        &'a self,
        previous: &'a Self,
    ) -> impl Iterator<Item = &'static str> + 'a {
        FILES.into_iter().filter_map(move |(file, name, _)| {
            (self.files[file as usize] != previous.files[file as usize]).then_some(name)
        })
    }
}

impl ShaderVariantKey {
    fn files(self) -> &'static [ShaderFile] {
        use ShaderFile::*;
        match self {
            Self::Primitive(BindingMode::Classic) => &[Object, Primitive, Classic],
            Self::Primitive(BindingMode::Bindless) => &[Object, Primitive, Bindless],
            Self::DirectionalShadow => &[Object, Shadow],
            Self::Skybox => &[Skybox],
        }
    }

    #[cfg(feature = "shader-reload")]
    pub(crate) fn changed(self, previous: &SceneShaderSources, next: &SceneShaderSources) -> bool {
        self.files()
            .iter()
            .any(|&file| previous.files[file as usize] != next.files[file as usize])
    }

    pub(crate) fn wgsl(self, sources: &SceneShaderSources) -> Option<String> {
        if !SHADER_VARIANTS.contains(&self) {
            return None;
        }
        let mut source = String::new();
        if self == Self::Primitive(BindingMode::Bindless) {
            source.push_str("enable wgpu_binding_array;\n");
        }
        for &file in self.files() {
            source.push_str(&sources.files[file as usize]);
            source.push('\n');
        }
        Some(source)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn reports_shader_variants() {
        println!(
            "Shader variants: {} / {VARIANT_LIMIT}",
            SHADER_VARIANTS.len()
        );
        for key in SHADER_VARIANTS {
            println!("  {key:?}");
        }
        let unique: HashSet<_> = SHADER_VARIANTS.iter().collect();
        assert_eq!(unique.len(), SHADER_VARIANTS.len());
        enforce_budget(VARIANT_LIMIT);
    }

    #[test]
    #[should_panic(expected = "shader variant budget exceeded")]
    fn rejects_over_budget_fixture() {
        enforce_budget(VARIANT_LIMIT + 1);
    }
}
