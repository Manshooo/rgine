use engine_platform::SurfaceSource;
use wgpu::{Backend, Backends, Instance, InstanceDescriptor};

use super::{RenderError, create_surface};
use crate::resources::ResourceTable;

// Filtering by the compiled features gives DX12 first on Windows, Metal first
// on Apple platforms and Vulkan first on Linux and Android.
const PREFERENCE: [Backend; 4] = [Backend::Dx12, Backend::Metal, Backend::Vulkan, Backend::Gl];

pub(super) struct Graphics {
    pub surface: wgpu::Surface<'static>,
    pub instance: Instance,
    pub adapter: wgpu::Adapter,
    pub resources: ResourceTable,
}

pub(super) fn open(source: &SurfaceSource, gpu_timing: bool) -> Result<Graphics, RenderError> {
    try_backends(
        Instance::enabled_backend_features(),
        Backends::from_env(),
        |backend| open_with_backend(source, backend, gpu_timing),
    )
}

fn open_with_backend(
    source: &SurfaceSource,
    backend: Backend,
    gpu_timing: bool,
) -> Result<Graphics, RenderError> {
    let descriptor = InstanceDescriptor::new_with_display_handle_from_env(Box::new(source.clone()));
    let instance = open_instance(descriptor, backend);
    let surface = create_surface(&instance, source)?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .map_err(|error| {
        RenderError(format!(
            "could not select a surface-compatible adapter: {error}"
        ))
    })?;
    if surface.get_default_config(&adapter, 1, 1).is_none() {
        return Err(RenderError(
            "adapter offers no usable surface configuration".into(),
        ));
    }
    let resources = ResourceTable::request(
        &adapter,
        std::env::var_os("RGINE_FORCE_CLASSIC").is_some_and(|value| value == "1"),
        gpu_timing,
    )?;
    Ok(Graphics {
        surface,
        instance,
        adapter,
        resources,
    })
}

fn open_instance(mut descriptor: InstanceDescriptor, backend: Backend) -> Instance {
    // Limit the instance itself so drivers for later candidates are loaded only on demand.
    descriptor.backends = backend.into();
    Instance::new(descriptor)
}

fn try_backends<T>(
    compiled: Backends,
    requested: Option<Backends>,
    mut open: impl FnMut(Backend) -> Result<T, RenderError>,
) -> Result<T, RenderError> {
    let eligible = compiled & requested.unwrap_or(Backends::all());
    let mut failures = Vec::new();
    for backend in PREFERENCE
        .into_iter()
        .filter(|&api| eligible.contains(api.into()))
    {
        match open(backend) {
            Ok(graphics) => return Ok(graphics),
            Err(error) => {
                tracing::warn!(?backend, %error, "graphics backend initialization failed");
                failures.push(format!("{backend:?} ({error})"));
            }
        }
    }
    if failures.is_empty() {
        let message = if requested.is_some() {
            format!(
                "WGPU_BACKEND selects no compiled native backend. Available backends are {compiled:?}"
            )
        } else {
            "no native graphics backend was compiled for this target".into()
        };
        return Err(RenderError(message));
    }
    Err(RenderError(format!(
        "could not initialize graphics with {}",
        failures.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tries_platform_preferences_and_stops_at_the_first_success() {
        for (compiled, expected) in [
            (
                Backends::DX12 | Backends::VULKAN | Backends::GL,
                Backend::Dx12,
            ),
            (
                Backends::METAL | Backends::VULKAN | Backends::GL,
                Backend::Metal,
            ),
            (Backends::VULKAN | Backends::GL, Backend::Vulkan),
        ] {
            let mut attempts = Vec::new();
            let selected = try_backends(compiled, None, |backend| {
                attempts.push(backend);
                Ok(backend)
            });
            assert_eq!(selected.unwrap(), expected);
            assert_eq!(attempts, [expected]);
        }
    }

    #[test]
    fn falls_back_in_order_when_an_attempt_fails() {
        let mut attempts = Vec::new();
        let selected = try_backends(
            Backends::DX12 | Backends::VULKAN | Backends::GL,
            None,
            |backend| {
                attempts.push(backend);
                if backend == Backend::Dx12 {
                    Err(RenderError("no compatible adapter".into()))
                } else {
                    Ok(backend)
                }
            },
        );
        assert_eq!(selected.unwrap(), Backend::Vulkan);
        assert_eq!(attempts, [Backend::Dx12, Backend::Vulkan]);
    }

    #[test]
    fn reports_failed_attempts_without_leaving_the_requested_set() {
        let mut attempts = Vec::new();
        let result = try_backends(
            Backends::DX12 | Backends::VULKAN | Backends::GL,
            Some(Backends::VULKAN | Backends::GL),
            |backend| {
                attempts.push(backend);
                let reason = if backend == Backend::Vulkan {
                    "adapter failed"
                } else {
                    "device failed"
                };
                Err::<(), _>(RenderError(reason.into()))
            },
        );
        assert_eq!(attempts, [Backend::Vulkan, Backend::Gl]);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("Vulkan (adapter failed)"));
        assert!(message.contains("Gl (device failed)"));
    }

    #[test]
    fn reports_an_unsupported_override_before_opening_an_instance() {
        for requested in [Backends::METAL, Backends::empty()] {
            let result = try_backends::<()>(Backends::DX12, Some(requested), |_| {
                panic!("an excluded backend must never be opened");
            });
            let message = result.unwrap_err().to_string();
            assert!(message.contains("WGPU_BACKEND"));
            assert!(message.contains("DX12"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires a Windows DX12 adapter"]
    fn automatically_opens_a_windows_dx12_device() {
        let info = try_backends(Instance::enabled_backend_features(), None, |backend| {
            // Reaching a later backend would mean that opening DX12 failed.
            assert_eq!(backend, Backend::Dx12);
            let instance = open_instance(
                InstanceDescriptor::new_without_display_handle_from_env(),
                backend,
            );
            let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
                .map_err(|error| RenderError(error.to_string()))?;
            let (_device, _queue) = pollster::block_on(adapter.request_device(&Default::default()))
                .map_err(|error| RenderError(error.to_string()))?;
            Ok(adapter.get_info())
        })
        .unwrap();
        assert_eq!(info.backend, Backend::Dx12);
        eprintln!(
            "automatic selection opened {} ({:?})",
            info.name, info.backend
        );
    }
}
