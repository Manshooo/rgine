use crate::resources::BindingMode;

const VARIANT_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum ShaderVariantKey {
    Primitive(BindingMode),
}

pub(crate) const SHADER_VARIANTS: &[ShaderVariantKey] = &[
    ShaderVariantKey::Primitive(BindingMode::Classic),
    ShaderVariantKey::Primitive(BindingMode::Bindless),
];

const _: () = enforce_budget(SHADER_VARIANTS.len());

const fn enforce_budget(count: usize) {
    assert!(count <= VARIANT_LIMIT, "shader variant budget exceeded");
}

impl ShaderVariantKey {
    pub(crate) fn binding_mode(self) -> BindingMode {
        match self {
            Self::Primitive(mode) => mode,
        }
    }

    pub(crate) fn wgsl(self) -> Option<String> {
        if !SHADER_VARIANTS.contains(&self) {
            return None;
        }
        let (extensions, fragment) = match self {
            Self::Primitive(BindingMode::Classic) => {
                ("", include_str!("scene/primitive_classic.wgsl"))
            }
            Self::Primitive(BindingMode::Bindless) => (
                "enable wgpu_binding_array;",
                include_str!("scene/primitive_bindless.wgsl"),
            ),
        };
        let shared = include_str!("scene/primitive.wgsl");
        Some(format!("{extensions}\n{shared}\n{fragment}"))
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
