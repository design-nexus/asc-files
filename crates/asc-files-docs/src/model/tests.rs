// SPDX-License-Identifier: MIT
//! Legacy's `sandbox_helper/model/tests.rs`, those about parsing, limits and drawing.

use super::*;

#[test]
fn large_binary_stl_is_accepted_and_over_limit_is_refused() {
    for (count, accepted) in [(1_620_000u32, true), (2_000_001, false)] {
        let mut bytes = vec![0; 84 + count as usize * 50];
        bytes[80..84].copy_from_slice(&count.to_le_bytes());
        let result = stl(&bytes);
        if accepted {
            assert_eq!(result.expect("81 MB STL").len(), count as usize);
        } else {
            assert!(
                result
                    .expect_err("triangle budget")
                    .contains("triangle limit")
            );
        }
    }
}

#[test]
fn binary_stl_with_solid_header_is_not_misread_as_ascii() {
    let mut bytes = vec![0; 84 + 50];
    bytes[..5].copy_from_slice(b"solid");
    bytes[80..84].copy_from_slice(&1u32.to_le_bytes());
    let points = [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]];
    for (vertex, point) in points.iter().enumerate() {
        for (axis, value) in point.iter().enumerate() {
            let at = 84 + 12 + vertex * 12 + axis * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    assert_eq!(stl(&bytes).expect("binary STL"), vec![points]);
}

#[test]
fn ascii_stl_parses_faces_and_rejects_non_finite_coordinates() {
    let source = b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid";
    assert_eq!(stl(source).expect("ASCII STL").len(), 1);
    assert!(stl(b"solid sample\nvertex 0 0 0\nvertex NaN 0 0\nvertex 0 1 0\nendsolid").is_err());
}

#[test]
fn package_renders_build_items_and_component_transforms() {
    let model = br#"<model><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object><object id="2"><components><component objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></components></object></resources><build><item objectid="2" transform="1 0 0 0 1 0 0 0 1 0 5 0"/></build></model>"#;
    assert_eq!(
        triangles_3mf(model).expect("3MF model"),
        vec![[[10., 5., 0.], [11., 5., 0.], [10., 6., 0.]]]
    );
}

#[test]
fn cyclic_components_are_bounded() {
    let model = br#"<model><resources><object id="1"><components><component objectid="1"/></components></object></resources><build><item objectid="1"/></build></model>"#;
    assert!(triangles_3mf(model).is_err());
}

#[test]
fn component_reference_and_expansion_budgets_reject_fanout() {
    let component = "<component objectid=\"1\"/>";
    let xml = format!(
        "<model><resources><object id=\"1\"><components>{}</components></object></resources><build><item objectid=\"1\"/></build></model>",
        component.repeat(MAX_MODEL_COMPONENT_REFERENCES + 1)
    );
    assert_eq!(
        triangles_3mf(xml.as_bytes()).expect_err("bounded failure"),
        "3MF component reference limit exceeded"
    );
    let xml = format!(
        "<model><resources><object id=\"1\"><components>{}</components></object></resources><build><item objectid=\"1\"/></build></model>",
        component.repeat(MAX_MODEL_COMPONENT_REFERENCES / 2)
    );
    assert_eq!(
        triangles_3mf(xml.as_bytes()).expect_err("bounded failure"),
        "3MF component expansion limit exceeded"
    );
}

#[test]
fn missing_geometry_references_are_rejected() {
    for xml in [
        "<model><build><item objectid=\"1\"/></build></model>",
        "<model><resources><object id=\"1\"><mesh><triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object></resources><build><item objectid=\"1\"/></build></model>",
    ] {
        assert!(
            triangles_3mf(xml.as_bytes())
                .expect_err("bounded failure")
                .contains("missing")
        );
    }
}

#[test]
fn overlapping_faces_exhaust_the_raster_budget() {
    let faces = vec![[[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]; 1000];
    assert!(
        shade(&faces, 800)
            .expect_err("bounded failure")
            .contains("rendering limit")
    );
}

#[test]
fn a_cube_is_drawn_square_on_a_clear_background() {
    // A unit cube's twelve triangles.
    let corners = |i: usize| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32];
    let quads = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    let faces: Vec<_> = quads
        .iter()
        .flat_map(|q| {
            [
                [corners(q[0]), corners(q[1]), corners(q[2])],
                [corners(q[0]), corners(q[2]), corners(q[3])],
            ]
        })
        .collect();
    let png = shade(&faces, 64).unwrap();
    let picture = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(picture.dimensions(), (64, 64));
    // The corners are clear, the middle is the model.
    assert_eq!(picture.get_pixel(0, 0)[3], 0);
    assert_eq!(picture.get_pixel(32, 32)[3], 255);
    // Its faces are lit differently.
    let shades: std::collections::HashSet<_> = picture
        .pixels()
        .filter(|p| p[3] == 255)
        .map(|p| p[0])
        .collect();
    assert!(shades.len() >= 2);
}
