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
    graph.add_pass("independent", &[], &[], 3);
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
