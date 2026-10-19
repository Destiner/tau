//! Opt-in native flowchart defaults; call `apply` before constructing the engine.
//!
//! Provenance: beautiful-mermaid 1.1.3 `src/layout-engine.ts` and `src/styles.ts`.
//! Its mergeEdges is a post-layout bundler, not ELK's mergeEdges option: the native
//! profile leaves ELK merging disabled. Post-layout bundling is a separate policy.
//! Requires the opt-in layered-profile adapter patch for effective 28/48 spacing.
//! Apply only for flowcharts: the root `layout` key also affects other families.
use serde_json::{Value, json};

pub(super) fn apply(config: &mut Value) {
    config["layout"] = json!("elk");
    config["elk"] = json!({
        "mergeEdges": false,
        "nodePlacementStrategy": "BRANDES_KOEPF",
        "nodePlacementAlignment": "BALANCED",
        "cycleBreakingStrategy": "GREEDY",
        "considerModelOrder": "NODES_AND_EDGES",
        "layeringStrategy": "NETWORK_SIMPLEX",
        "orientFeedbackEdges": false,
        "straightenEdges": false,
        "lineHops": false
    });
    let flow = config["flowchart"]
        .as_object_mut()
        .expect("flowchart config");
    for (key, value) in [
        ("diagramPadding", json!(12)),
        ("padding", json!(20)),
        ("nodeSpacing", json!(28)),
        ("rankSpacing", json!(48)),
        (
            "nativeElkProfile",
            json!({
                "edgeEdge":12, "edgeNode":10, "edgeEdgeBetweenLayers":12,
                "edgeNodeBetweenLayers":12, "edgeLabel":2,
                "thoroughness":3, "centerContent":true, "inlineCenterLabels":true
            }),
        ),
        ("nativeBundleEdges", json!(true)),
        (
            "nativePostCompaction",
            json!("LEFT_RIGHT_CONSTRAINT_LOCKING"),
        ),
        ("nativeAlignLayers", json!(true)),
        ("nativeEdgeBoundsPadding", json!(40)),
        ("nativeEdgeLabelBoundsRight", json!(60)),
        ("nativeEdgeLabelBoundsBottom", json!(20)),
        ("nativeNodePaddingHorizontal", json!(20)),
        ("nativeNodePaddingVertical", json!(10)),
        ("nativeDiamondExtra", json!(24)),
        ("nativeNodeLineHeight", json!(1.3)),
        ("nativeEdgeLineHeight", json!(1.3)),
        ("nativeEdgeLabelPaddingHorizontal", json!(4)),
        ("nativeEdgeLabelPaddingVertical", json!(3)),
        ("nativeMinNodeWidth", json!(60)),
        ("nativeMinNodeHeight", json!(36)),
        (
            "nativeLabelProfile",
            json!({
                "node": {"fontSize":13,"fontWeight":"500"},
                "edge": {"fontSize":11,"fontWeight":"400"},
                "group": {"fontSize":12,"fontWeight":"600"}
            }),
        ),
    ] {
        flow.insert(key.into(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_metrics_and_unrelated_family_configuration_are_preserved() {
        let mut config = json!({"flowchart":{"htmlLabels":false},"sequence":{"actorMargin":60}});
        apply(&mut config);
        assert_eq!(config["layout"], "elk");
        assert_eq!(config["sequence"]["actorMargin"], 60);
        assert_eq!(config["flowchart"]["htmlLabels"], false);
        assert_eq!(
            config["flowchart"]["nativeLabelProfile"]["node"]["fontWeight"],
            "500"
        );
        assert_eq!(
            config["flowchart"]["nativeLabelProfile"]["edge"]["fontSize"],
            11
        );
        assert_eq!(
            config["flowchart"]["nativeLabelProfile"]["group"]["fontWeight"],
            "600"
        );
        assert!(config.get("nodes").is_none());
    }
}
