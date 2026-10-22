//! The importers never panic, whatever the input: errors are values.

use freekerf_import::dxf::{self, DxfOptions};
use freekerf_import::svg::{self, SvgOptions};
use proptest::prelude::*;

fn path_data() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just("M".to_owned()),
            Just("m".to_owned()),
            Just("L".to_owned()),
            Just("l".to_owned()),
            Just("H".to_owned()),
            Just("V".to_owned()),
            Just("C".to_owned()),
            Just("S".to_owned()),
            Just("Q".to_owned()),
            Just("T".to_owned()),
            Just("A".to_owned()),
            Just("Z".to_owned()),
            Just(",".to_owned()),
            Just(" ".to_owned()),
            (-1000.0..1000.0f64).prop_map(|v| format!("{v}")),
            (-100i32..100).prop_map(|v| format!("{v}.5.5")),
            Just("1e3".to_owned()),
            Just("0".to_owned()),
        ],
        0..40,
    )
    .prop_map(|v| v.concat())
}

proptest! {
    #[test]
    fn svg_paths_never_panic(d in path_data(), transform in "[a-z(), 0-9.-]{0,30}", w in "[0-9a-z%.]{0,6}") {
        let text = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="10mm" viewBox="0 0 10 10"><g transform="{transform}"><path d="{d}" stroke="red"/><polygon points="{d}"/><rect width="{w}" height="2" rx="1"/><ellipse rx="1" ry="0"/></g></svg>"#
        );
        let _ = svg::import(text.as_bytes(), &SvgOptions::default());
        let _ = svg::import(text.as_bytes(), &SvgOptions { scale_to_mm: Some(10.0), user_offset: (1.0, 2.0), legacy_bezier: true, fonts: None });
    }

    #[test]
    fn svg_texts_never_panic(d in path_data(), size in "[0-9a-z%.]{0,6}", offset in "[0-9%.-]{0,5}", body in "[ a-zA-Z0-9\\n]{0,12}") {
        let text = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><path id="p" d="{d}"/><text font-size="{size}" style="letter-spacing:{offset}"><tspan y="1">{body}</tspan><textPath href="#p" startOffset="{offset}">{body}</textPath></text></svg>"##
        );
        let fonts = SvgOptions { fonts: Some(freekerf_text::Fonts::builtin()), ..SvgOptions::default() };
        let _ = svg::import(text.as_bytes(), &fonts);
    }

    #[test]
    fn svg_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let _ = svg::import(&bytes, &SvgOptions::default());
    }

    #[test]
    fn dxf_never_panics(pairs in prop::collection::vec((prop::sample::select(vec![0, 2, 8, 10, 11, 20, 21, 40, 41, 42, 50, 51, 62, 70, 71, 230, 420]),
                                                        prop_oneof![Just("LINE".to_owned()), Just("CIRCLE".to_owned()), Just("SPLINE".to_owned()),
                                                                    Just("LWPOLYLINE".to_owned()), Just("INSERT".to_owned()), Just("SECTION".to_owned()),
                                                                    Just("ENTITIES".to_owned()), Just("BLOCKS".to_owned()), Just("BLOCK".to_owned()),
                                                                    (-100.0..100.0f64).prop_map(|v| v.to_string()), Just("x".to_owned())]), 0..60)) {
        let mut text = String::from("0\nSECTION\n2\nENTITIES\n");
        for (code, value) in pairs {
            text.push_str(&format!("{code}\n{value}\n"));
        }
        let _ = dxf::import(text.as_bytes(), &DxfOptions::default());
        let _ = dxf::import(text.as_bytes(), &DxfOptions { spline_tolerance: 0.0 });
    }
}
