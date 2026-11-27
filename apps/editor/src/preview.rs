use engine_core::glam::{Mat4, Vec3};
use engine_ecs::Component;
use engine_render::{
    canvas::Vec2,
    scene::{Mesh, MeshKey, MeshRegistry, Scene, SceneCamera, SceneObject},
};
use engine_scene::ObjectId;

use crate::document::Document;

const NODE_COLOR: [f32; 4] = [0.12, 0.42, 0.67, 1.0];
const SELECTED_COLOR: [f32; 4] = [1.0, 0.62, 0.12, 1.0];
const VERTICAL_FOV: f32 = std::f32::consts::FRAC_PI_3;

#[derive(Clone, Copy, Component)]
struct PreviewNode {
    id: ObjectId,
    position: Vec3,
}

pub(crate) struct Preview {
    pub meshes: MeshRegistry,
    pub scene: Scene,
    mesh: MeshKey,
    object_ids: Vec<ObjectId>,
    highlighted: Option<ObjectId>,
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
            object_ids: Vec::new(),
            highlighted: None,
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
        if self.highlighted != document.selected {
            for (&id, object) in self.object_ids.iter().zip(&mut self.scene.objects) {
                if Some(id) == self.highlighted {
                    object.color = NODE_COLOR;
                } else if Some(id) == document.selected {
                    object.color = SELECTED_COLOR;
                }
            }
            self.highlighted = document.selected;
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

    pub fn pick(
        &mut self,
        document: &Document<'_>,
        region: [u32; 4],
        pointer: Vec2,
    ) -> Option<ObjectId> {
        let [x, y, width, height] = region;
        // Use the same pixel centre and physical viewport as the rasterizer.
        let pixel = pointer.floor() + Vec2::splat(0.5);
        let origin = Vec2::new(x as f32, y as f32);
        let size = Vec2::new(width as f32, height as f32);
        if !origin.cmple(pixel).all() || !pixel.cmplt(origin + size).all() {
            return None;
        }
        if self.update(document, Some(region)).is_some() {
            return None;
        }
        let inverse_clip = self
            .scene
            .camera?
            .clip_from_world([width, height])?
            .inverse();
        let uv = (pixel - origin) / size;
        let ndc = Vec2::new(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y);
        let near = inverse_clip.project_point3(ndc.extend(0.0));
        let far = inverse_clip.project_point3(ndc.extend(1.0));
        let ray = far - near;
        if !near.is_finite() || !ray.is_finite() || ray == Vec3::ZERO {
            return None;
        }

        let mut closest = None;
        let mut distance = f32::INFINITY;
        for (&id, object) in self.object_ids.iter().zip(&self.scene.objects) {
            // Every preview node uses the same unit cube with a translation-only pose.
            let local_origin = near - object.transform.w_axis.truncate();
            if let Some(hit) = cube_hit(local_origin, ray)
                && hit < distance
            {
                closest = Some(id);
                distance = hit;
            }
        }
        closest
    }

    fn rebuild(&mut self, document: &Document<'_>) -> Result<(), String> {
        self.object_ids.clear();
        self.highlighted = None;
        self.scene.objects.clear();
        self.scene.camera = None;
        self.bounds = None;

        let mut fallback_index = 0;
        let compilation = document.truth().compile(|id, object, _| {
            let position = if let Some(position) = object.property("position") {
                let position = position
                    .read::<[f32; 3]>()
                    .map(Vec3::from_array)
                    .map_err(|error| format!("position: {error}"))?;
                if !position.is_finite() {
                    return Err("position must contain finite coordinates".to_owned());
                }
                position
            } else {
                let index = fallback_index;
                fallback_index += 1;
                Vec3::new((index % 8) as f32 * 1.5, 0.0, (index / 8) as f32 * 1.5)
            };
            Ok(PreviewNode { id, position })
        });
        let world = compilation
            .map_err(|error| error.to_string())?
            .into_compiled()
            .instantiate(|world, entity, &node, _| {
                world
                    .insert(entity, node)
                    .map(|_| ())
                    .map_err(|_| "preview entity disappeared during loading")
            })
            .map_err(|error| error.to_string())?;
        world.query::<&PreviewNode>().for_each(|_, node| {
            let position = node.position;
            self.bounds = Some(self.bounds.map_or((position, position), |(min, max)| {
                (min.min(position), max.max(position))
            }));
            self.object_ids.push(node.id);
            self.scene.objects.push(SceneObject {
                mesh: self.mesh,
                transform: Mat4::from_translation(position),
                color: NODE_COLOR,
            });
        });
        Ok(())
    }
}

fn cube_hit(origin: Vec3, ray: Vec3) -> Option<f32> {
    let mut enter = 0.0_f32;
    let mut exit = 1.0_f32;
    for (origin, direction) in origin.to_array().into_iter().zip(ray.to_array()) {
        if direction == 0.0 {
            if !(-0.5..=0.5).contains(&origin) {
                return None;
            }
            continue;
        }
        let a = (-0.5 - origin) / direction;
        let b = (0.5 - origin) / direction;
        enter = enter.max(a.min(b));
        exit = exit.min(a.max(b));
        if enter > exit {
            return None;
        }
    }
    Some(enter)
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
        document.selected = None;
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
        write_positions(&path, &positions);
        document.open(&path).unwrap();
        for region @ [x, y, width, height] in [[10, 20, 860, 360], [10, 20, 250, 510]] {
            assert_eq!(preview.update(&document, Some(region)), None);
            let camera = preview.scene.camera.unwrap();
            let clip = camera.clip_from_world([width, height]).unwrap();
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
            for (&id, position) in preview.object_ids.clone().iter().zip(positions) {
                let ndc = clip.project_point3(position);
                let pixel = Vec2::new(
                    x as f32 + (ndc.x + 1.0) * 0.5 * width as f32,
                    y as f32 + (1.0 - ndc.y) * 0.5 * height as f32,
                );
                assert_eq!(preview.pick(&document, region, pixel), Some(id));
            }
        }
    }

    #[test]
    fn picks_the_front_cube_and_ignores_deleted_geometry() {
        let registry = registry();
        let mut document = Document::new(&registry);
        let mut preview = Preview::new();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("overlap.scene.ron");
        write_positions(&path, &[Vec3::ZERO, 2.0 * Vec3::new(0.8, 0.65, 1.0)]);
        document.open(&path).unwrap();
        let ids: Vec<_> = document.truth().objects().map(|(id, _)| id).collect();
        let region = [20, 30, 601, 401];
        let middle = Vec2::new(320.5, 230.5);

        assert_eq!(preview.pick(&document, region, middle), Some(ids[1]));
        document.selected = Some(ids[1]);
        document.delete(ids[1]).unwrap();
        assert_eq!(document.selected, None);
        assert_eq!(preview.pick(&document, region, middle), Some(ids[0]));
        assert_eq!(preview.pick(&document, region, Vec2::new(20.0, 30.0)), None);
        assert_eq!(
            preview.pick(&document, region, Vec2::new(19.9, 200.0)),
            None
        );
    }

    fn write_positions(path: &std::path::Path, positions: &[Vec3]) {
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
        std::fs::write(path, source).unwrap();
    }
}
