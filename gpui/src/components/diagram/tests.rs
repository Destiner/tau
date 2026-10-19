use super::*;
#[test]
fn er_default_direction_profile() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let d = renderer
            .render(
                "erDiagram\n A ||--o{ B : contains",
                DiagramTheme::ayu(dark),
                &op(),
            )
            .unwrap();
        assert_eq!(d.semantic_model()["direction"], "LR");
        assert!(d.view_box().width > d.view_box().height);
        let d = renderer
            .render(
                "erDiagram\n direction BT\n A ||--o{ B : contains",
                DiagramTheme::ayu(dark),
                &op(),
            )
            .unwrap();
        assert_eq!(d.semantic_model()["direction"], "BT");
        assert!(d.view_box().height > d.view_box().width);
    }
}
#[test]
fn compact_profiles_fonts_geometry_and_resolved_palette_all_families() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let theme = DiagramTheme::ayu(dark);
        let appearance = profile::Appearance::resolve(theme);
        for (name, family, width, height, tolerance) in [
            ("FLOWCHART", DiagramFamily::Flowchart, 111.892, 145.8, 1.),
            ("state", DiagramFamily::State, 151.363, 374.1, 1.),
            ("SEQUENCE", DiagramFamily::Sequence, 280., 160., 0.01),
            ("class", DiagramFamily::Class, 200., 256., 0.01),
            ("er", DiagramFamily::Er, 590.042, 136., 3.),
            ("xy", DiagramFamily::Xy, 750., 492., 0.01),
        ] {
            let fixture = fixtures().into_iter().find(|f| f["name"] == name).unwrap();
            let source = fixture["source"].as_str().unwrap();
            let config = profile::config(theme, family);
            let metadata = Engine::new()
                .with_site_config(MermaidConfig::from_value(config.clone()))
                .parse_metadata_sync(source)
                .unwrap();
            assert_eq!(
                DiagramFamily::detect(&metadata.diagram_type).unwrap(),
                family
            );
            assert_eq!(
                config["layout"],
                if family == DiagramFamily::Flowchart {
                    "elk"
                } else {
                    "dagre"
                }
            );
            assert_eq!(config["fontSize"], false);
            assert_eq!(config["themeVariables"]["fontFamily"], profile::SANS);
            assert_eq!(
                config["themeVariables"]["nodeBorder"],
                profile::color(appearance.stroke)
            );
            assert_eq!(
                config["themeVariables"]["signalTextColor"],
                profile::color(appearance.muted)
            );
            let d = renderer.render(source, theme, &op()).unwrap();
            assert_eq!(d.family(), family);
            assert!(
                (d.view_box().width - width).abs() < tolerance,
                "{name}: {:?}",
                d.view_box()
            );
            assert!(
                (d.view_box().height - height).abs() < tolerance,
                "{name}: {:?}",
                d.view_box()
            );
            let doc = roxmltree::Document::parse(d.svg()).unwrap();
            let css = doc
                .descendants()
                .filter(|n| n.has_tag_name("style"))
                .filter_map(|n| n.text())
                .collect::<String>();
            if family != DiagramFamily::Xy {
                assert!(css.contains(&format!(
                    "font-family:TauInter;font-size:{}px;fill:{}",
                    if matches!(family, DiagramFamily::Class | DiagramFamily::Er) {
                        11
                    } else {
                        13
                    },
                    profile::color(theme.text)
                )));
                assert!(css.contains(&format!("stroke:{}", profile::color(appearance.stroke))));
                assert!(!css.contains("font-family:Menlo !important"));
            } else {
                assert!(d.svg().contains("font-family=\"TauInter\""));
            }
            let has_rect_height = |height: f64| {
                doc.descendants().any(|n| {
                    n.has_tag_name("rect")
                        && n.attribute("height")
                            .and_then(|h| h.parse::<f64>().ok())
                            .is_some_and(|h| (h - height).abs() < 0.01)
                })
            };
            let has_role = |family: &str, size: u8, weight: u16| {
                doc.descendants().any(|n| {
                    n.has_tag_name("text")
                        && n.attribute("style").is_some_and(|s| {
                            s.contains(&format!("font-family:{family}"))
                                && s.contains(&format!("font-size:{size}px"))
                                && s.contains(&format!("font-weight:{weight}"))
                        })
                })
            };
            match family {
                DiagramFamily::Flowchart => {
                    assert_eq!(config["elk"]["mergeEdges"], false);
                    assert_eq!(config["flowchart"]["nativeElkProfile"]["thoroughness"], 3);
                    assert_eq!(
                        config["flowchart"]["nativeLabelProfile"]["edge"]["fontSize"],
                        11
                    );
                    assert_eq!(config["flowchart"]["nativeNodeLineHeight"], 1.3);
                    assert!(has_rect_height(36.9));
                    assert!(css.contains("font-size:13px;font-weight:500"));
                    assert!(css.contains("font-size:11px;font-weight:400"));
                }
                DiagramFamily::State => {
                    assert_eq!(config["state"]["presentationProfile"], "compact");
                    assert_eq!(config["state"]["presentation"]["fontFamily"], profile::SANS);
                    assert_eq!(config["state"]["presentation"]["edgeFontSize"], 11);
                    assert!(has_rect_height(36.9));
                    assert!(doc.descendants().any(|n| n.has_tag_name("circle")
                        && n.attribute("r") == Some("8")
                        && n.attribute("fill") == Some(profile::color(theme.text).as_str())));
                    assert_eq!(d.semantic_model()["edges"].as_array().unwrap().len(), 3);
                }
                DiagramFamily::Sequence => {
                    assert_eq!(config["sequence"]["presentationProfile"], "compact");
                    assert_eq!(config["sequence"]["presentation"]["actorCenterGap"], 140);
                    assert!(has_role(profile::SANS, 13, 500));
                    assert!(has_role(profile::SANS, 11, 400));
                    assert!(has_rect_height(40.));
                    assert_eq!(
                        doc.descendants()
                            .filter(|n| n.has_tag_name("rect")
                                && n.attribute("class") == Some("actor actor-top"))
                            .count(),
                        2
                    );
                }
                DiagramFamily::Class => {
                    assert_eq!(config["class"]["presentationProfile"], "compact");
                    assert_eq!(config["class"]["memberFontFamily"], profile::MONO);
                    assert!(has_role(profile::SANS, 13, 700));
                    assert!(has_role(profile::MONO, 11, 400));
                    assert!(has_rect_height(32.));
                    let marker = doc
                        .descendants()
                        .find(|n| {
                            n.has_tag_name("marker")
                                && n.attribute("id")
                                    .is_some_and(|s| s.ends_with("extensionStart"))
                        })
                        .unwrap();
                    let marker_id = marker.attribute("id").unwrap();
                    assert!(doc.descendants().any(|n| n.attribute("marker-start")
                        == Some(format!("url(#{marker_id})").as_str())));
                    assert!(
                        marker
                            .descendants()
                            .any(|n| n.has_tag_name("polygon") && n.attribute("points").is_some())
                    );
                    assert!(css.contains(&format!(
                        "fill:{} !important; stroke:{} !important",
                        profile::color(theme.canvas),
                        profile::color(theme.text)
                    )));
                    assert_eq!(d.semantic_model()["relations"][0]["id1"], "Session");
                }
                DiagramFamily::Er => {
                    assert_eq!(config["er"]["presentationProfile"], "compact");
                    assert_eq!(config["er"]["attributeFontFamily"], profile::MONO);
                    assert!(has_role(profile::SANS, 13, 700));
                    assert!(has_role(profile::SANS, 9, 600));
                    assert!(has_role(profile::MONO, 11, 400));
                    assert!(has_rect_height(34.) && has_rect_height(56.));
                    assert_eq!(
                        d.semantic_model()["entities"]["SESSION"]["attributes"][0]["keys"],
                        json!(["PK"])
                    );
                    assert!(css.contains(&format!(
                        "marker.er circle {{ fill:{} !important",
                        profile::color(theme.canvas)
                    )));
                }
                DiagramFamily::Xy => {
                    assert!(doc.descendants().any(|n| n.has_tag_name("text")
                        && n.attribute("font-size") == Some("14")
                        && n.attribute("font-weight") == Some("400")));
                    assert_eq!(d.semantic_model()["plots"].as_array().unwrap().len(), 2);
                }
            }
        }
    }
}

#[test]
fn metadata_selects_family_through_frontmatter_and_directives() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        for (source, family) in [
            (
                "---\ntitle: sequenceDiagram is not the type\n---\n%% classDiagram\ngraph TD\n A-->B",
                DiagramFamily::Flowchart,
            ),
            (
                "%%{init: {\"fontFamily\":\"TauInter\"}}%%\n%% flowchart TD\nclassDiagram\n Parent <|-- Child",
                DiagramFamily::Class,
            ),
            (
                "%% flowchart TD\nsequenceDiagram\n A->>B: flowchart TD",
                DiagramFamily::Sequence,
            ),
        ] {
            let d = renderer
                .render(source, DiagramTheme::ayu(dark), &op())
                .unwrap_or_else(|e| panic!("{source}: {e:?}"));
            assert_eq!(d.family(), family);
        }
    }
}

#[test]
fn flowchart_real_layout_budget_exhaustion_and_natural_topology_bounds() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let theme = DiagramTheme::ayu(dark);
        assert_eq!(
            renderer
                .render_with_layout_budget("flowchart TD\n A-->B", theme, &op(), 1)
                .unwrap_err(),
            DiagramError::ComplexityLimit
        );
        for (name, width, height) in [
            ("REPORTED_FLOWCHART", 257.886, 953.54),
            ("COMPACT_DOTTED_FLOWCHART", 580.219, 554.464),
        ] {
            let f = fixtures().into_iter().find(|f| f["name"] == name).unwrap();
            let d = renderer
                .render(f["source"].as_str().unwrap(), theme, &op())
                .unwrap();
            assert!(
                (d.view_box().width / width - 1.).abs() < 0.03,
                "{name}: {:?}",
                d.view_box()
            );
            assert!(
                (d.view_box().height / height - 1.).abs() < 0.03,
                "{name}: {:?}",
                d.view_box()
            );
        }
    }
}

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures.json")).unwrap()
}
fn op() -> Operation {
    Operation::new(Duration::from_secs(10))
}
fn normalize(s: &str) -> String {
    s.replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("\\n", "\n")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}
fn shape(s: &str) -> &str {
    match s {
        "square" => "rectangle",
        "round" => "rounded",
        "stadium" => "stadium",
        "circle" => "circle",
        "doublecircle" => "doublecircle",
        "diamond" => "diamond",
        "hexagon" => "hexagon",
        "odd" => "asymmetric",
        "subroutine" => "subroutine",
        "cylinder" => "cylinder",
        "lean_right" => "parallelogram",
        "lean_left" => "parallelogram-alt",
        "trapezoid" => "trapezoid",
        "inv_trapezoid" => "trapezoid-alt",
        _ => s,
    }
}

#[test]
fn fixture_semantics_and_offline_raster_both_schemes() {
    let renderer = DiagramRenderer::new().unwrap();
    let mut families = std::collections::HashSet::new();
    let mut quoted = 0;
    for fixture in fixtures().iter().filter(|f| {
        !f["name"].as_str().unwrap().contains("bad")
            && !f["name"].as_str().unwrap().starts_with("security")
            && f["name"] != "untrusted"
    }) {
        let name = fixture["name"].as_str().unwrap();
        for dark in [false, true] {
            let diagram = renderer
                .render(
                    fixture["source"].as_str().unwrap(),
                    DiagramTheme::ayu(dark),
                    &op(),
                )
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            families.insert(format!("{:?}", diagram.family()));
            let b = diagram.view_box();
            assert!(b.width > 0. && b.height > 0.);
            let m = diagram.semantic_model();
            match name {
                "SEQUENCE" => {
                    assert_eq!(m["actorOrder"], json!(["Tau", "Pi"]));
                    assert_eq!(m["messages"][0]["from"], "Tau");
                    assert_eq!(m["messages"][0]["to"], "Pi");
                    assert_eq!(m["messages"][0]["message"], "prompt");
                }
                "state" => {
                    assert_eq!(m["nodes"].as_array().unwrap().len(), 4);
                    assert_eq!(m["edges"].as_array().unwrap().len(), 3);
                    assert_eq!(m["edges"][1]["start"], "Idle");
                    assert_eq!(m["edges"][1]["end"], "Active");
                    assert_eq!(m["edges"][1]["label"], "prompt");
                }
                "class" => {
                    assert_eq!(m["classes"].as_object().unwrap().len(), 2);
                    assert_eq!(m["relations"][0]["id1"], "Session");
                    assert_eq!(m["relations"][0]["id2"], "LiveSession");
                    assert_eq!(
                        m["classes"]["Session"]["methods"][0]["displayText"],
                        "+load()"
                    );
                }
                "er" => {
                    assert_eq!(m["entities"].as_object().unwrap().len(), 2);
                    assert_eq!(m["relationships"].as_array().unwrap().len(), 1);
                    assert_eq!(m["relationships"][0]["roleA"], "contains");
                    assert_eq!(
                        m["entities"]["SESSION"]["attributes"][0]["keys"],
                        json!(["PK"])
                    );
                }
                "xy" => {
                    assert_eq!(m["plots"].as_array().unwrap().len(), 2);
                    assert_eq!(m["xAxis"]["categories"], json!(["Jan", "Feb", "Mar"]));
                    assert_eq!(m["plots"][0]["values"], json!([10., 20., 30.]));
                    assert_eq!(m["plots"][1]["type"], "line");
                }
                _ => (),
            }
            if let Some(nodes) = fixture["expected"]["nodes"].as_array() {
                let actual = diagram.semantic_model()["nodes"].as_array().unwrap();
                assert_eq!(actual.len(), nodes.len(), "{name} nodes");
                for expected in nodes {
                    let node = actual.iter().find(|n| n["id"] == expected["id"]).unwrap();
                    assert_eq!(
                        normalize(node["label"].as_str().unwrap()),
                        normalize(expected["label"].as_str().unwrap()),
                        "{name} label"
                    );
                    assert_eq!(
                        shape(node["shape"].as_str().unwrap_or("square")),
                        expected["shape"].as_str().unwrap(),
                        "{name} shape"
                    );
                }
                let edges = fixture["expected"]["edges"].as_array().unwrap();
                let actual = diagram.semantic_model()["edges"].as_array().unwrap();
                assert_eq!(actual.len(), edges.len(), "{name} edges");
                for (a, e) in actual.iter().zip(edges) {
                    assert_eq!(a["from"], e["source"], "{name}");
                    assert_eq!(a["to"], e["target"], "{name}");
                    assert_eq!(
                        a["stroke"],
                        if e["style"] == "dotted" {
                            "dotted"
                        } else {
                            "normal"
                        },
                        "{name} edge style"
                    );
                    assert_eq!(
                        normalize(a["label"].as_str().unwrap_or("")),
                        normalize(e["label"].as_str().unwrap_or("")),
                        "{name}"
                    );
                }
                if name == "REPORTED_FLOWCHART" {
                    assert_eq!((nodes.len(), edges.len()), (7, 8));
                }
                if name == "COMPACT_DOTTED_FLOWCHART" {
                    assert_eq!((nodes.len(), edges.len()), (11, 13));
                }
            }
            let image = renderer
                .rasterize(
                    &diagram,
                    RasterCamera {
                        viewport: b,
                        backing_scale: 1.,
                        zoom: 1.,
                    },
                    &op(),
                )
                .unwrap();
            assert_eq!(
                image.rgba.len(),
                image.width as usize * image.height as usize * 4
            );
            assert!(
                image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{name} painted"
            );
            assert!(
                image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 0),
                "{name} transparent"
            );
        }
        if name.starts_with("quoted_") && !name.contains("multiline") {
            quoted += 1;
        }
    }
    assert_eq!(families.len(), 6);
    assert_eq!(quoted, 14);
}
#[test]
fn materialized_theme_survives_family_config_merge() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let theme = DiagramTheme::ayu(dark);
        let d = renderer
            .render(
                "xychart-beta\n x-axis [A,B]\n y-axis 0 --> 20\n bar [10,20]",
                theme,
                &op(),
            )
            .unwrap();
        let config = &d.semantic_model()["config"];
        assert_eq!(config["fontFamily"], "TauInter");
        assert_eq!(config["themeVariables"]["fontSize"], "13px");
        assert_eq!(
            config["themeVariables"]["primaryColor"],
            profile::color(theme.panel_raised)
        );
        assert_eq!(
            config["themeVariables"]["primaryTextColor"],
            profile::color(theme.text)
        );
        assert_eq!(
            config["themeVariables"]["arrowheadColor"],
            profile::color(theme.accent_text)
        );
        assert_eq!(config["flowchart"]["nodeSpacing"], 28);
        assert_eq!(config["flowchart"]["rankSpacing"], 48);
        let flow = renderer
            .render("flowchart TD\n A[Prompt]-->B[Answer]", theme, &op())
            .unwrap();
        assert!(flow.svg().contains("font-size:13px"));
        // Source's generic family layout is 111.892 x 145.8 for this graph.
        assert!((flow.view_box().width - 111.892).abs() < 5.);
        assert!((flow.view_box().height - 145.8).abs() < 6.);
    }
}
#[test]
fn sequence_dimensions_roles_and_no_footer_actors() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let d = renderer
            .render(
                "sequenceDiagram\n Tau->>Pi: prompt",
                DiagramTheme::ayu(dark),
                &op(),
            )
            .unwrap();
        let doc = roxmltree::Document::parse(d.svg()).unwrap();
        let actors: Vec<_> = doc
            .descendants()
            .filter(|n| {
                n.has_tag_name("text") && n.attribute("class").is_some_and(|c| c.contains("actor"))
            })
            .collect();
        assert_eq!(actors.len(), 2);
        for actor in actors {
            assert!(actor.attribute("style").unwrap().contains("font-size:13px"));
        }
        let msg = doc
            .descendants()
            .find(|n| n.attribute("class") == Some("messageText"))
            .unwrap();
        assert!(msg.attribute("style").unwrap().contains("font-size:11px"));
        assert!((d.view_box().width - 280.).abs() < 1.);
        assert!((d.view_box().height - 160.).abs() < 0.01);
        assert!(d.svg().contains("stroke-dasharray:6 4"));
    }
}
#[test]
fn xy_generic_presentation_roles_and_compatibility_palette() {
    let renderer = DiagramRenderer::new().unwrap();
    for dark in [false, true] {
        let theme = DiagramTheme::ayu(dark);
        let d=renderer.render("xychart-beta\n title \"Latency\"\n x-axis [Jan, Feb, Mar]\n y-axis \"ms\" 0 --> 100\n bar [10,20,30]\n line [15,25,35]",theme,&op()).unwrap();
        assert_eq!((d.view_box().width, d.view_box().height), (750., 492.));
        let doc = roxmltree::Document::parse(d.svg()).unwrap();
        let texts: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("text"))
            .collect();
        for (s, size, weight) in [
            ("Latency", "18", "600"),
            ("ms", "15", "500"),
            ("Jan", "14", "400"),
            ("Bar 1", "14", "400"),
            ("Line 1", "14", "400"),
        ] {
            let n = texts.iter().find(|n| n.text() == Some(s)).unwrap();
            assert_eq!(n.attribute("font-size"), Some(size));
            assert_eq!(n.attribute("font-weight"), Some(weight));
        }
        assert_eq!(
            doc.descendants()
                .filter(|n| n.attribute("class") == Some("xy-point"))
                .count(),
            3
        );
        assert!(
            doc.descendants()
                .filter(|n| n.attribute("class") == Some("xy-grid"))
                .count()
                > 300
        );
        let line = doc
            .descendants()
            .find(|n| n.attribute("class") == Some("xy-line"))
            .unwrap();
        assert_eq!(line.attribute("stroke"), Some("#0d5ba5"));
        assert_eq!(line.attribute("stroke-width"), Some("2.5"));
        let bar = doc
            .descendants()
            .find(|n| n.attribute("class") == Some("xy-bar"))
            .unwrap();
        assert_eq!(
            bar.attribute("fill"),
            Some(profile::color(presentation::mix(theme.canvas, theme.text, 0.25)).as_str())
        );
        security::validate(d.svg()).unwrap();
        for source in [
            "xychart-beta horizontal\n x-axis [A,B,C]\n y-axis -10 --> 10\n bar [-5,0,8]\n line [2,2,-3]",
            "xychart-beta\n x-axis 0 --> 2\n y-axis 0 --> 20\n line [1,8,3]",
        ] {
            let other = renderer.render(source, theme, &op()).unwrap();
            security::validate(other.svg()).unwrap();
            assert!(!other.svg().contains("NaN"));
        }
    }
}
#[test]
fn xy_inert_serialization_limits_and_cancellation_both_schemes() {
    let renderer = DiagramRenderer::new().unwrap();
    let source = "xychart-beta\n x-axis [A,B,C]\n y-axis 0 --> 10\n bar [1,2,3]";
    for dark in [false, true] {
        let d = renderer
            .render(source, DiagramTheme::ayu(dark), &op())
            .unwrap();
        let mut chart: merman::diagrams::xychart::XyChartDiagramRenderModel =
            serde_json::from_value(d.semantic_model().clone()).unwrap();
        chart.title =
            Some("<script onclick='evil()'> & <image href='https://example.com'/ >".into());
        let svg = xy::render(&chart, DiagramTheme::ayu(dark), &renderer.fonts, &op()).unwrap();
        security::validate(&svg).unwrap();
        let doc = roxmltree::Document::parse(&svg).unwrap();
        assert_eq!(
            doc.descendants()
                .find(|n| n.has_tag_name("title"))
                .unwrap()
                .text(),
            chart.title.as_deref()
        );
        assert!(
            !doc.descendants()
                .any(|n| n.has_tag_name("script") || n.has_tag_name("image"))
        );
        let cancelled = op();
        cancelled.cancel();
        assert_eq!(
            xy::render(&chart, DiagramTheme::ayu(dark), &renderer.fonts, &cancelled).unwrap_err(),
            DiagramError::Cancelled
        );
        chart.plots[0].values = vec![1.; 4097];
        assert_eq!(
            xy::render(&chart, DiagramTheme::ayu(dark), &renderer.fonts, &op()).unwrap_err(),
            DiagramError::ComplexityLimit
        );
    }
}
#[test]
fn requested_monospace_shares_measurement_and_raster_face() {
    let fonts = fonts::Fonts::new().unwrap();
    let style = merman::svg::TextStyle {
        font_family: Some(profile::MONO.into()),
        font_size: 11.,
        ..Default::default()
    };
    let narrow = fonts.measure("iiii", &style);
    let wide = fonts.measure("WWWW", &style);
    assert!(narrow > 0.);
    assert!((narrow - wide).abs() < 0.01);
    let options = fonts.options();
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 20'><text x='0' y='15' font-family='Menlo' font-size='11'>iiiiWWWW</text></svg>";
    let tree = resvg::usvg::Tree::from_str(svg, &options).unwrap();
    assert!(!tree.root().children().is_empty());
}
#[test]
fn marker_policy_preserves_hollow_and_filled_semantics() {
    for dark in [false, true] {
        let css = presentation::css(DiagramTheme::ayu(dark));
        assert!(!css.contains(".marker,.arrowheadPath"));
        assert!(css.contains("marker.extension path"));
        assert!(css.contains("marker.composition path"));
        assert!(css.contains("marker.er circle"));
    }
}
#[test]
fn malformed_and_partial_retain_source() {
    let renderer = DiagramRenderer::new().unwrap();
    for fixture in fixtures()
        .iter()
        .filter(|f| f["name"].as_str().unwrap().contains("bad"))
    {
        assert!(
            renderer
                .render(
                    fixture["source"].as_str().unwrap(),
                    DiagramTheme::ayu(true),
                    &op()
                )
                .is_err(),
            "{}",
            fixture["name"]
        );
    }
    assert!(
        renderer
            .render(
                "flowchart TD\n A[unfinished",
                DiagramTheme::ayu(true),
                &op()
            )
            .is_err()
    );
}
#[test]
fn bounds_cancellation_and_deadlines() {
    let renderer = DiagramRenderer::new().unwrap();
    let theme = DiagramTheme::ayu(true);
    assert_eq!(
        renderer
            .render(&"α".repeat(8001), theme, &op())
            .unwrap_err(),
        DiagramError::SourceLimit
    );
    // 8000 UTF-16 units, but more than 8000 UTF-8 bytes: admitted, valid label.
    let unicode = format!("flowchart TD\n A[\"{}\"]", "α".repeat(2600));
    assert_ne!(
        renderer.render(&unicode, theme, &op()).err(),
        Some(DiagramError::SourceLimit)
    );
    let cancelled = op();
    cancelled.cancel();
    assert_eq!(
        renderer
            .render("graph TD\n A-->B", theme, &cancelled)
            .unwrap_err(),
        DiagramError::Cancelled
    );
    assert_eq!(
        renderer
            .render("graph TD\n A-->B", theme, &Operation::new(Duration::ZERO))
            .unwrap_err(),
        DiagramError::Deadline
    );
    let many = format!(
        "flowchart TD\n{}",
        (0..270)
            .map(|i| format!("N{i}-->N{}\n", i + 1))
            .collect::<String>()
    );
    assert_eq!(
        renderer.render(&many, theme, &op()).unwrap_err(),
        DiagramError::ComplexityLimit
    );
    let diagram = renderer.render("graph TD\n A-->B", theme, &op()).unwrap();
    for (scale, zoom) in [(f64::NAN, 1.), (0., 1.), (2., 10000.)] {
        assert!(matches!(
            renderer.rasterize(
                &diagram,
                RasterCamera {
                    viewport: diagram.view_box(),
                    backing_scale: scale,
                    zoom
                },
                &op()
            ),
            Err(DiagramError::RasterLimit)
        ));
    }
    assert!(matches!(
        renderer.rasterize(
            &diagram,
            RasterCamera {
                viewport: diagram.view_box(),
                backing_scale: 1.,
                zoom: 1.
            },
            &cancelled
        ),
        Err(DiagramError::Cancelled)
    ));
}
#[test]
fn final_gate_rejects_resources_active_content_and_limits() {
    for body in [
        "<script/>",
        "<foreignObject/>",
        "<image href='data:image/png;base64,AAA'/>",
        "<a href='https://example.com'/>",
        "<text onclick='evil()'/>",
        "<style>@import 'https://example.com';</style>",
        "<rect fill='url(https://example.com)'/>",
        "<style>text{fill:u\\72l(https://example.com)}</style>",
        "<use href='#x'/>",
        "<marker overflow='url(https://example.com)'/>",
        "<rect overflow='visible'/>",
    ] {
        let svg =
            format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'>{body}</svg>");
        assert_eq!(security::validate(&svg), Err(DiagramError::UnsafeResource));
    }
    assert_eq!(
        security::validate("<svg viewBox='0 0 99999 2'/>"),
        Err(DiagramError::DimensionLimit)
    );
    assert_eq!(
        security::validate(&" ".repeat(MAX_SVG_BYTES + 1)),
        Err(DiagramError::OutputLimit)
    );
    assert_eq!(
        security::validate(&format!(
            "<svg viewBox='0 0 2 2'>{}</svg>",
            "<g/>".repeat(MAX_SVG_ELEMENTS)
        )),
        Err(DiagramError::OutputLimit)
    );
    let renderer = DiagramRenderer::new().unwrap();
    for f in fixtures()
        .iter()
        .filter(|f| f["name"].as_str().unwrap().starts_with("security"))
    {
        if let Ok(d) = renderer.render(
            f["source"].as_str().unwrap(),
            DiagramTheme::ayu(true),
            &op(),
        ) {
            security::validate(d.svg()).unwrap();
        }
    }
}
#[test]
fn local_marker_overflow_is_inert_and_raster_bounds_stay_owned() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><defs><marker id='tip' markerWidth='12' markerHeight='10' overflow='visible'><path d='M-12 -5 L0 0 L-12 5 Z'/></marker></defs><path d='M10 50 L90 50' marker-end='url(#tip)'/></svg>";
    let bounds = security::validate(svg).unwrap();
    assert_eq!(bounds.width, 100.);
    assert_eq!(bounds.height, 100.);
}

#[test]
fn fonts_weights_unicode_and_camera_resolution() {
    let renderer = DiagramRenderer::new().unwrap();
    for weight in ["400", "500", "600", "650", "700"] {
        let style = merman::svg::TextStyle {
            font_weight: Some(weight.into()),
            font_size: 14.,
            ..Default::default()
        };
        for text in ["α", "✓", "≤", "Inter α ✓ ≤"] {
            assert!(renderer.fonts.measure(text, &style) > 0.);
        }
    }
    let d = renderer
        .render(
            "flowchart LR\n A[\"α ✓ ≤\"]-->B[bold]",
            DiagramTheme::ayu(true),
            &op(),
        )
        .unwrap();
    let a = renderer
        .rasterize(
            &d,
            RasterCamera {
                viewport: d.view_box(),
                backing_scale: 1.,
                zoom: 1.,
            },
            &op(),
        )
        .unwrap();
    let b = renderer
        .rasterize(
            &d,
            RasterCamera {
                viewport: d.view_box(),
                backing_scale: 2.,
                zoom: 2.,
            },
            &op(),
        )
        .unwrap();
    assert!(b.width >= a.width * 4 - 3 && b.height >= a.height * 4 - 3);
    fn send_sync<T: Send + Sync>() {}
    send_sync::<DiagramRenderer>();
    send_sync::<Diagram>();
    send_sync::<Operation>();
    let cancel = op();
    let handle = cancel.clone();
    handle.cancel();
    let worker = std::thread::spawn(move || {
        renderer.render("flowchart TD\n A-->B", DiagramTheme::ayu(false), &cancel)
    });
    assert_eq!(worker.join().unwrap().unwrap_err(), DiagramError::Cancelled);
}

#[test]
fn cancellation_from_another_thread_and_raster_deadline() {
    let renderer = DiagramRenderer::new().unwrap();
    let operation = op();
    let cancel = operation.clone();
    let source = format!(
        "flowchart TD\n{}",
        (0..180)
            .map(|i| format!("N{i}-->N{}\n", i + 1))
            .collect::<String>()
    );
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(3));
        cancel.cancel();
    });
    assert_eq!(
        renderer
            .render(&source, DiagramTheme::ayu(true), &operation)
            .unwrap_err(),
        DiagramError::Cancelled
    );
    canceller.join().unwrap();
    let diagram = renderer
        .render("flowchart TD\n A-->B", DiagramTheme::ayu(true), &op())
        .unwrap();
    assert!(matches!(
        renderer.rasterize(
            &diagram,
            RasterCamera {
                viewport: diagram.view_box(),
                backing_scale: 2.,
                zoom: 1.
            },
            &Operation::new(Duration::ZERO)
        ),
        Err(DiagramError::Deadline)
    ));
}

#[test]
fn negative_viewbox_and_cropped_camera_do_not_fetch_resources() {
    let renderer = DiagramRenderer::new().unwrap();
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='-20 -10 100 80'><rect x='-20' y='-10' width='20' height='20' fill='#bfbdb6'/></svg>";
    let view_box = security::validate(svg).unwrap();
    let diagram = Diagram {
        family: DiagramFamily::Flowchart,
        model: Value::Null,
        svg: Arc::from(svg),
        view_box,
    };
    let image = renderer
        .rasterize(
            &diagram,
            RasterCamera {
                viewport: ViewBox {
                    x: -20.,
                    y: -10.,
                    width: 20.,
                    height: 20.,
                },
                backing_scale: 2.,
                zoom: 1.,
            },
            &op(),
        )
        .unwrap();
    assert_eq!((image.width, image.height), (40, 40));
    assert!(image.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    let options = renderer.fonts.options();
    assert!(
        (options.image_href_resolver.resolve_string)("https://example.com/pixel.png", &options)
            .is_none()
    );
}
