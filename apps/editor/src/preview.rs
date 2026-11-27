use engine_core::glam::{Mat4, Vec3};
use engine_ecs::Component;
use engine_render::scene::{Mesh, MeshKey, MeshRegistry, Scene, SceneCamera, SceneObject};

use crate::document::Document;

const NODE_COLOR: [f32; 4] = [0.12, 0.42, 0.67, 1.0];
const VERTICAL_FOV: f32 = std::f32::consts::FRAC_PI_3;

#[derive(Component)]
struct PreviewPosition(Vec3);

pub(crate) struct Preview {
    pub meshes: MeshRegistry,
    pub scene: Scene,
    mesh: MeshKey,
    revision: Option<u64>,
    bounds: Option<(Vec3, Vec3)>,
    compile_error: Option<String>,
}

impl Preview {
    pub fn new() -> Self {
        let mut meshes = MeshRegistry::default();
        let mesh = meshes.insert(Mesh::cube());
        Self {
            meshes,
            scene: Scene::default(),
            mesh,
            revision: None,
            bounds: None,
            compile_error: None,
        }
    }

    pub fn update(&mut self, document: &Document<'_>, region: Option<[u32; 4]>) -> Option<&str> {
        if self.revision != Some(document.revision()) {
            self.compile_error = self.rebuild(document).err();
            self.revision = Some(document.revision());
        }
        if let Some(error) = &self.compile_error {
            return Some(error);
        }
        self.scene.camera = None;
        if let Some(((min, max), [_, _, width, height])) = self.bounds.zip(region) {
            let center = min * 0.5 + max * 0.5;
            let radius = ((max - center).length() + 0.87).max(1.0);
            self.scene.camera = frame_camera(center, radius, width as f32 / height as f32);
            if self.scene.camera.is_none() {
                return Some("The scene positions exceed the preview camera range.");
            }
        }
        None
    }

    fn rebuild(&mut self, document: &Document<'_>) -> Result<(), String> {
        self.scene.objects.clear();
        self.scene.camera = None;
        self.bounds = None;

        let mut fallback_index = 0;
        let compilation = document.truth().compile(|_, object, _| {
            if let Some(position) = object.property("position") {
                let position = position
                    .read::<[f32; 3]>()
                    .map(Vec3::from_array)
                    .map_err(|error| format!("position: {error}"))?;
                if !position.is_finite() {
                    return Err("position must contain finite coordinates".to_owned());
                }
                return Ok(position);
            }

            let index = fallback_index;
            fallback_index += 1;
            Ok(Vec3::new(
                (index % 8) as f32 * 1.5,
                0.0,
                (index / 8) as f32 * 1.5,
            ))
        });
        let world = compilation
            .map_err(|error| error.to_string())?
            .into_compiled()
            .instantiate(|world, entity, &position, _| {
                world
                    .insert(entity, PreviewPosition(position))
                    .map(|_| ())
                    .map_err(|_| "preview entity disappeared during loading")
            })
            .map_err(|error| error.to_string())?;
        world.query::<&PreviewPosition>().for_each(|_, position| {
            let position = position.0;
            self.bounds = Some(self.bounds.map_or((position, position), |(min, max)| {
                (min.min(position), max.max(position))
            }));
            self.scene.objects.push(SceneObject {
                mesh: self.mesh,
                transform: Mat4::from_translation(position),
                color: NODE_COLOR,
            });
        });
        Ok(())
    }
}

fn frame_camera(center: Vec3, radius: f32, aspect: f32) -> Option<SceneCamera> {
    let half_vertical_fov = VERTICAL_FOV * 0.5;
    let half_horizontal_fov = (half_vertical_fov.tan() * aspect).atan();
    let distance = radius * 1.2 / half_horizontal_fov.min(half_vertical_fov).sin();
    let direction = Vec3::new(0.8, 0.65, 1.0).normalize();
    // Build the view from a fixed orientation to preserve it for distant levels.
    let view = Mat4::look_at_rh(direction * distance, Vec3::ZERO, Vec3::Y)
        * Mat4::from_translation(-center);
    let far = distance + radius * 2.0;
    (view.is_finite() && far.is_finite()).then_some(SceneCamera {
        view,
        vertical_fov: VERTICAL_FOV,
        near: (distance - radius * 1.5).max(0.05),
        far,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::registry;

    #[test]
    fn rebuilds_preview_after_hierarchy_edits_and_opening_a_saved_level() {
        let registry = registry();
        let mut document = Document::new(&registry);
        let mut preview = Preview::new();
        let region = Some([200, 140, 600, 400]);
        assert_eq!(preview.update(&document, region), None);
        assert!(preview.scene.objects.is_empty());
        document.add().unwrap();
        let root = document.selected.unwrap();
        document.add().unwrap();
        assert_eq!(preview.update(&document, region), None);
        let saved_objects = preview.scene.objects.clone();
        assert_eq!(saved_objects.len(), 2);
        assert_ne!(saved_objects[0].transform, saved_objects[1].transform);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.scene.ron");
        document.save(&path).unwrap();
        document.delete(root).unwrap();
        assert_eq!(preview.update(&document, region), None);
        assert!(preview.scene.objects.is_empty());
        document.open(&path).unwrap();
        assert_eq!(preview.update(&document, region), None);
        assert_eq!(preview.scene.objects, saved_objects);
        assert_eq!(preview.update(&document, None), None);
        assert!(preview.scene.camera.is_none());
        assert_eq!(preview.update(&document, region), None);
        assert!(preview.scene.camera.is_some());
    }

    #[test]
    fn frames_authored_positions_in_wide_and_narrow_regions() {
        let registry = registry();
        let mut document = Document::new(&registry);
        let mut preview = Preview::new();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("positioned.scene.ron");
        let positions = [Vec3::new(200.0, 4.0, -2.0), Vec3::new(210.0, 2.0, -5.0)];
        let mut source = String::from("(schema_version: 2, objects: {");
        for (index, position) in positions.iter().enumerate() {
            let [x, y, z] = position.to_array();
            source.push_str(&format!(
                "\"00000000-0000-0000-0000-{:012}\": (
                    object_type: (name: \"rgine.level_node\", version: 1),
                    parent: None, properties: {{\"position\": ({x}, {y}, {z})}},
                ),",
                index + 1
            ));
        }
        source.push_str("})");
        std::fs::write(&path, source).unwrap();
        document.open(&path).unwrap();
        for (width, height) in [(860, 360), (250, 510)] {
            assert_eq!(
                preview.update(&document, Some([10, 20, width, height])),
                None
            );
            let camera = preview.scene.camera.unwrap();
            let clip = Mat4::perspective_rh(
                camera.vertical_fov,
                width as f32 / height as f32,
                camera.near,
                camera.far,
            ) * camera.view;
            assert_eq!(preview.scene.objects.len(), positions.len());
            for (object, position) in preview.scene.objects.iter().zip(positions) {
                assert_eq!(object.transform, Mat4::from_translation(position));
                for x in [-0.5, 0.5] {
                    for y in [-0.5, 0.5] {
                        for z in [-0.5, 0.5] {
                            let ndc = clip.project_point3(position + Vec3::new(x, y, z));
                            assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0);
                            assert!((0.0..1.0).contains(&ndc.z));
                        }
                    }
                }
            }
        }
    }
}
