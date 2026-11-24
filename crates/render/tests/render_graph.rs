use std::convert::Infallible;

use engine_render::graph::{GraphError, RenderGraph};

#[test]
fn executes_dependencies_before_consumers_with_a_stable_ready_order() {
    let mut graph = RenderGraph::new();
    let source = graph.import("scene input");
    let depth = graph.transient("depth");
    let frame = graph.import("backbuffer");
    graph.add_pass("main", &[depth], &[frame], 1);
    graph.add_pass("depth", &[source], &[depth], 2);
    graph.add_side_effect_pass("independent", &[], &[], 3);
    graph.export(frame);

    let graph = graph.compile().unwrap();
    assert_eq!(graph.exports(), &[frame]);
    let mut executed = Vec::new();
    graph
        .execute::<Infallible>(|pass| {
            executed.push(*pass.data());
            if pass.name() == "main" {
                assert_eq!(pass.reads(), &[depth]);
                assert_eq!(pass.writes(), &[frame]);
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(executed, [2, 1, 3]);
}

#[test]
fn skips_passes_that_contribute_to_no_output_or_side_effect() {
    let mut graph = RenderGraph::new();
    let frame = graph.import("backbuffer");
    let unused = graph.transient("unused");
    graph.add_pass("clear", &[], &[frame], ());
    graph.add_pass("disconnected", &[], &[], ());
    graph.add_pass("unused producer", &[], &[unused], ());
    graph.add_pass("unused reader", &[unused, frame], &[], ());
    graph.export(frame);

    let graph = graph.compile().unwrap();
    let mut executed = Vec::new();
    graph
        .execute::<Infallible>(|pass| {
            executed.push(pass.name().to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(executed, ["clear"]);
}

#[test]
fn retains_the_transitive_producer_chain_for_an_export() {
    let mut graph = RenderGraph::new();
    let frame = graph.import("backbuffer");
    let depth = graph.transient("depth");
    let meshes = graph.transient("meshes");
    graph.add_pass("main", &[depth], &[frame], 3);
    graph.add_pass("depth", &[meshes], &[depth], 2);
    graph.add_pass("prepare meshes", &[], &[meshes], 1);
    graph.export(frame);

    let graph = graph.compile().unwrap();
    let mut executed = Vec::new();
    graph
        .execute::<Infallible>(|pass| {
            executed.push(*pass.data());
            Ok(())
        })
        .unwrap();
    assert_eq!(executed, [1, 2, 3]);
}

#[test]
fn retains_a_side_effect_pass_and_its_dependencies_without_an_export() {
    let mut graph = RenderGraph::new();
    let buffer = graph.transient("capture buffer");
    graph.add_side_effect_pass("readback", &[buffer], &[], 2);
    graph.add_pass("capture", &[], &[buffer], 1);

    let graph = graph.compile().unwrap();
    let mut executed = Vec::new();
    graph
        .execute::<Infallible>(|pass| {
            executed.push(*pass.data());
            Ok(())
        })
        .unwrap();
    assert_eq!(executed, [1, 2]);
}

#[test]
fn an_exported_import_can_have_no_live_passes() {
    let mut graph = RenderGraph::new();
    let input = graph.import("input");
    graph.add_pass("unused reader", &[input], &[], ());
    graph.export(input);

    let graph = graph.compile().unwrap();
    assert_eq!(graph.exports(), &[input]);
    graph
        .execute::<Infallible>(|_| panic!("no pass should execute"))
        .unwrap();
}

#[test]
fn rejects_a_read_or_export_of_a_transient_without_a_producer() {
    for export in [false, true] {
        let mut graph = RenderGraph::new();
        let depth = graph.transient("depth");
        let consumer = if export {
            graph.export(depth);
            "graph output"
        } else {
            graph.add_pass("main", &[depth], &[], ());
            "main"
        };
        assert_eq!(
            graph.compile().err(),
            Some(GraphError::MissingProducer {
                resource: "depth".into(),
                consumer: consumer.into(),
            })
        );
    }
}

#[test]
fn reports_the_cycle_without_including_a_pass_waiting_on_it() {
    let mut graph = RenderGraph::new();
    let depth = graph.transient("depth");
    let light = graph.transient("light");
    graph.add_pass("main", &[depth], &[], ());
    graph.add_pass("depth pass", &[light], &[depth], ());
    graph.add_pass("light pass", &[depth], &[light], ());

    let error = graph.compile().err().expect("graph should contain a cycle");
    assert_eq!(
        error.to_string(),
        "render graph dependency cycle: depth pass -> light pass -> depth pass"
    );
}

#[test]
fn rejects_ambiguous_producers() {
    let mut graph = RenderGraph::new();
    let frame = graph.import("backbuffer");
    graph.add_pass("opaque", &[], &[frame], ());
    graph.add_pass("sky", &[], &[frame], ());
    assert_eq!(
        graph.compile().err(),
        Some(GraphError::MultipleProducers {
            resource: "backbuffer".into(),
            first: "opaque".into(),
            second: "sky".into(),
        })
    );
}

#[test]
fn rejects_handles_from_another_graph_even_when_the_indices_match() {
    let mut other = RenderGraph::<()>::new();
    let foreign = other.import("foreign");
    let mut graph = RenderGraph::new();
    graph.import("local");
    graph.add_pass("main", &[foreign], &[], ());
    assert_eq!(
        graph.compile().err(),
        Some(GraphError::ForeignResource {
            consumer: "main".into(),
        })
    );
}
