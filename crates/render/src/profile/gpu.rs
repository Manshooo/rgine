use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use super::{GpuPassTime, GpuSample};

const SLOTS: usize = 4;
const MAX_PASSES: usize = 4;
const QUERY_COUNT: u32 = 2 * MAX_PASSES as u32;
const BUFFER_SIZE: u64 = QUERY_COUNT as u64 * wgpu::QUERY_SIZE as u64;

pub(crate) struct GpuProfiler {
    period_ns: f64,
    slots: [Slot; SLOTS],
}

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: Option<Pending>,
}

struct Pending {
    frame_id: usize,
    names: [&'static str; MAX_PASSES],
    count: usize,
    signal: Receiver<Result<(), wgpu::BufferAsyncError>>,
}

pub(crate) struct PassQueries<'a> {
    slot: &'a mut Slot,
    names: [&'static str; MAX_PASSES],
    count: usize,
    overflow: bool,
}

impl GpuProfiler {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let period_ns = f64::from(queue.get_timestamp_period());
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
            || !period_ns.is_finite()
            || period_ns <= 0.0
        {
            return None;
        }
        let slots = std::array::from_fn(|_| Slot {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: QUERY_COUNT,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame timestamps resolve"),
                size: BUFFER_SIZE,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame timestamps readback"),
                size: BUFFER_SIZE,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            pending: None,
        });
        Some(Self { period_ns, slots })
    }

    pub fn begin(&mut self) -> Option<PassQueries<'_>> {
        let slot = self.slots.iter_mut().find(|slot| slot.pending.is_none())?;
        Some(PassQueries {
            slot,
            names: [""; MAX_PASSES],
            count: 0,
            overflow: false,
        })
    }

    pub fn poll(&mut self, device: &wgpu::Device, finish: bool) -> Vec<(usize, GpuSample)> {
        if self.slots.iter().all(|slot| slot.pending.is_none()) {
            return Vec::new();
        }
        let mode = if finish {
            wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(1)),
            }
        } else {
            wgpu::PollType::Poll
        };
        let _ = device.poll(mode);
        self.slots
            .iter_mut()
            .filter_map(|slot| slot.take_result(self.period_ns))
            .collect()
    }
}

impl PassQueries<'_> {
    pub fn writes(&mut self, name: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let Some(entry) = self.names.get_mut(self.count) else {
            self.overflow = true;
            return None;
        };
        *entry = name;
        let start = 2 * self.count as u32;
        self.count += 1;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.slot.queries,
            beginning_of_pass_write_index: Some(start),
            end_of_pass_write_index: Some(start + 1),
        })
    }

    pub fn resolve(self, encoder: &mut wgpu::CommandEncoder, frame_id: usize) {
        if self.count == 0 || self.overflow {
            return;
        }
        let count = 2 * self.count as u32;
        let bytes = u64::from(count) * u64::from(wgpu::QUERY_SIZE);
        encoder.resolve_query_set(&self.slot.queries, 0..count, &self.slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.slot.resolve, 0, &self.slot.readback, 0, bytes);
        let (sender, signal) = mpsc::channel();
        encoder.map_buffer_on_submit(
            &self.slot.readback,
            wgpu::MapMode::Read,
            0..bytes,
            move |result| {
                let _ = sender.send(result);
            },
        );
        self.slot.pending = Some(Pending {
            frame_id,
            names: self.names,
            count: self.count,
            signal,
        });
    }
}

impl Slot {
    fn take_result(&mut self, period_ns: f64) -> Option<(usize, GpuSample)> {
        let result = match self.pending.as_ref()?.signal.try_recv() {
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => false,
            Ok(result) => result.is_ok(),
        };
        let pending = self.pending.take().expect("readback pending");
        let mut sample = GpuSample::Incomplete;
        if result {
            let bytes = (2 * pending.count) as u64 * u64::from(wgpu::QUERY_SIZE);
            if let Ok(data) = self.readback.get_mapped_range(0..bytes) {
                let timestamp = |bytes: &[u8]| u64::from_le_bytes(bytes.try_into().unwrap());
                let pairs = data
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|pair| (timestamp(&pair[..8]), timestamp(&pair[8..])));
                if let Some(passes) = pass_times(&pending.names[..pending.count], pairs, period_ns)
                {
                    sample = GpuSample::Complete { passes };
                }
            }
            self.readback.unmap();
        }
        Some((pending.frame_id, sample))
    }
}

fn pass_times(
    names: &[&'static str],
    pairs: impl Iterator<Item = (u64, u64)> + Clone,
    period_ns: f64,
) -> Option<Vec<GpuPassTime>> {
    let first = pairs.clone().map(|(start, _)| start).min()?;
    let nanos = |ticks: u64| (ticks as f64 * period_ns).round() as u64;
    names
        .iter()
        .zip(pairs)
        .map(|(&name, (start, end))| {
            Some(GpuPassTime {
                name,
                offset_ns: nanos(start - first),
                duration_ns: nanos(end.checked_sub(start)?),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_gpu_readback_on_a_device_without_timestamp_queries() {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::NOOP;
        desc.backend_options.noop.enable = true;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        assert!(GpuProfiler::new(&device, &queue).is_none());
    }

    #[test]
    fn uses_queue_period_and_preserves_offsets_across_pass_order() {
        let base = u64::MAX - 100;
        let names = ["shadow", "opaque"];
        let pairs = [(base + 10, base + 13), (base, base + 4)];
        let times = pass_times(&names, pairs.into_iter(), 2.5).unwrap();
        assert_eq!(times[0].offset_ns, 25);
        assert_eq!(times[0].duration_ns, 8);
        assert_eq!(times[1].offset_ns, 0);
        assert_eq!(times[1].duration_ns, 10);
        assert!(pass_times(&names[..1], [(9, 1)].into_iter(), 2.5).is_none());
    }
}
