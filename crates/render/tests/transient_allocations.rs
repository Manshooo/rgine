use std::convert::Infallible;

use engine_render::graph::{RenderGraph, ResourceLifetime};

#[test]
fn retained_passes_reuse_one_backend_allocation_for_two_transients() {
    let mut graph = RenderGraph::with_transient_descriptors();
    let first = graph.transient_with_descriptor("first", 64);
    let second = graph.transient_with_descriptor("second", 64);
    let culled = graph.transient_with_descriptor("culled", 256);
    let ready = graph.transient("first read complete");
    graph.add_pass("produce second", &[ready], &[second], ());
    graph.add_pass("consume first", &[first], &[ready], ());
    graph.add_pass("produce first", &[], &[first], ());
    graph.add_pass("unused", &[first, second], &[culled], ());
    graph.add_side_effect_pass("consume second", &[second], &[], ());

    let graph = graph.compile().unwrap();
    assert_eq!(
        graph.resource_lifetime(first),
        Some(ResourceLifetime {
            first_use: 0,
            last_use: 1
        })
    );
    assert_eq!(
        graph.resource_lifetime(second),
        Some(ResourceLifetime {
            first_use: 2,
            last_use: 3
        })
    );
    assert_eq!(graph.resource_lifetime(culled), None);
    assert_eq!(graph.transient_allocation(culled), None);
    assert_eq!(graph.transient_allocation(ready), None);
    assert_eq!(graph.transient_allocation_count(), 1);

    let mut requested_sizes = Vec::new();
    let bindings = graph
        .allocate_transients(|&size| {
            requested_sizes.push(size);
            Ok::<_, Infallible>(vec![0_u8; size].into_boxed_slice())
        })
        .unwrap();
    assert_eq!(requested_sizes, [64]);

    let mut write_targets = Vec::new();
    graph
        .execute::<Infallible>(|pass| {
            for &resource in pass.writes() {
                if let Some(slot) = graph.transient_allocation(resource) {
                    write_targets.push(bindings.get(slot).unwrap().as_ptr());
                }
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(write_targets.len(), 2);
    assert_eq!(write_targets[0], write_targets[1]);
}

#[test]
fn same_pass_accesses_overlap_and_distinct_descriptors_cannot_share_storage() {
    let mut graph = RenderGraph::with_transient_descriptors();
    let first = graph.transient_with_descriptor("first", 64);
    let second = graph.transient_with_descriptor("second", 64);
    let larger = graph.transient_with_descriptor("larger", 128);
    let input = graph.import("input");
    let history = graph.persistent("history");
    let ready = graph.transient("second read complete");
    graph.add_pass("produce first", &[input, history], &[first], ());
    graph.add_pass("replace contents", &[first], &[second], ());
    graph.add_pass("consume second", &[second], &[ready], ());
    graph.add_pass("produce larger", &[ready], &[larger], ());
    graph.export(larger);

    let graph = graph.compile().unwrap();
    let first_slot = graph.transient_allocation(first).unwrap();
    let second_slot = graph.transient_allocation(second).unwrap();
    let larger_slot = graph.transient_allocation(larger).unwrap();
    assert_ne!(first_slot, second_slot);
    assert_ne!(first_slot, larger_slot);
    assert_ne!(second_slot, larger_slot);
    assert_eq!(graph.transient_allocation_count(), 3);
    assert_eq!(graph.transient_allocation(input), None);
    assert_eq!(graph.transient_allocation(history), None);
}

#[test]
fn exported_transients_hold_separate_storage_until_execution_ends() {
    let mut graph = RenderGraph::with_transient_descriptors();
    let first = graph.transient_with_descriptor("first output", 64);
    let second = graph.transient_with_descriptor("second output", 64);
    graph.add_pass("first output", &[], &[first], ());
    graph.add_pass("second output", &[], &[second], ());
    graph.export(first);
    graph.export(second);

    let graph = graph.compile().unwrap();
    assert_eq!(
        graph.resource_lifetime(first),
        Some(ResourceLifetime {
            first_use: 0,
            last_use: 2
        })
    );
    assert_ne!(
        graph.transient_allocation(first),
        graph.transient_allocation(second)
    );
    assert_eq!(graph.transient_allocation_count(), 2);
}

#[test]
fn allocation_bindings_are_scoped_to_the_graph_that_created_them() {
    let build_graph = || {
        let mut graph = RenderGraph::with_transient_descriptors();
        let resource = graph.transient_with_descriptor("output", 64);
        graph.add_pass("output", &[], &[resource], ());
        graph.export(resource);
        (graph.compile().unwrap(), resource)
    };
    let (graph, resource) = build_graph();
    let (other_graph, other_resource) = build_graph();
    let bindings = graph
        .allocate_transients(|&size| Ok::<_, Infallible>(size))
        .unwrap();
    assert_eq!(
        bindings.get(graph.transient_allocation(resource).unwrap()),
        Some(&64)
    );
    assert_eq!(graph.resource_lifetime(other_resource), None);
    assert_eq!(graph.transient_allocation(other_resource), None);
    let other_slot = other_graph.transient_allocation(other_resource).unwrap();
    assert_eq!(bindings.get(other_slot), None);
}
