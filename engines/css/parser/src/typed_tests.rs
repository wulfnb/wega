//! Tests for `typed`.
use super::*;

mod parsing {
    use super::*;

    #[test]
    fn ported_longhands_parse_to_typed_values() {
        use PropertyDeclaration as P;
        assert_eq!(
            PropertyDeclaration::parse("display", "flex"),
            Parsed::Typed(P::Display(Display::Flex))
        );
        assert_eq!(
            PropertyDeclaration::parse("margin-left", "12px"),
            Parsed::Typed(P::Margin(Side::Left, Length::Px(12.0)))
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "calc(2 + 1)"),
            Parsed::Typed(P::ZIndex(Some(3)))
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "auto"),
            Parsed::Typed(P::ZIndex(None))
        );
        assert_eq!(
            PropertyDeclaration::parse("clear", "both"),
            Parsed::Typed(P::Clear(true, true))
        );
    }

    #[test]
    fn invalid_values_are_reported_and_cascade_keywords_are_deferred() {
        assert_eq!(PropertyDeclaration::parse("width", "wide"), Parsed::Invalid);
        assert_eq!(
            PropertyDeclaration::parse("float", "middle"),
            Parsed::Invalid
        );
        assert_eq!(
            PropertyDeclaration::parse("z-index", "high"),
            Parsed::Invalid
        );
        for value in [
            "inherit",
            "initial",
            "unset",
            "var(--w)",
            "calc(var(--w) * 2)",
            "calc(100vw - 20px)",
            "min(50vh, 300px)",
        ] {
            assert_eq!(PropertyDeclaration::parse("width", value), Parsed::Deferred);
        }
        assert_eq!(
            PropertyDeclaration::parse("font-size", "12px"),
            Parsed::Deferred
        );
        assert_eq!(
            PropertyDeclaration::parse("margin", "1px"),
            Parsed::Deferred
        );
    }

    #[test]
    fn stylesheets_report_invalid_declarations() {
        let sheet = crate::StyleSheet::parse("p { width: wide; float: middle; height: 4px }");
        assert!(
            sheet
                .unsupported
                .iter()
                .any(|u| u == "invalid value:width: wide")
        );
        assert!(
            sheet
                .unsupported
                .iter()
                .any(|u| u == "invalid value:float: middle")
        );
        assert_eq!(
            sheet.rules[0].declarations[2].typed,
            Parsed::Typed(PropertyDeclaration::Height(Length::Px(4.0)))
        );
    }

    #[test]
    fn every_family_parses_specified_values() {
        use PropertyDeclaration as P;
        let typed = |p, v| match PropertyDeclaration::parse(p, v) {
            Parsed::Typed(t) => t,
            other => panic!("{p}: {v} -> {other:?}"),
        };
        assert_eq!(
            typed("color", "#ff0000"),
            P::Color(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })
        );
        assert_eq!(
            typed("border-left-width", "thick"),
            P::BorderWidth(Side::Left, Length::Px(5.0))
        );
        assert_eq!(
            typed("border-top-width", "2em"),
            P::BorderWidth(Side::Top, Length::Em(2.0))
        );
        assert_eq!(
            typed("border-bottom-style", "hidden"),
            P::BorderStyleHidden(Side::Bottom, true)
        );
        assert_eq!(typed("font-weight", "bold"), P::FontWeight(700.0));
        assert_eq!(
            typed("line-height", "1.5"),
            P::LineHeight(LineHeight::Factor(1.5))
        );
        assert_eq!(typed("opacity", "50%"), P::Opacity(0.5));
        assert_eq!(
            typed("gap", "4px 8px"),
            P::Gap(Length::Px(4.0), Length::Px(8.0))
        );
        assert_eq!(typed("flex-grow", "2"), P::FlexGrow(2.0));
        assert_eq!(
            typed("transform-origin", "left"),
            P::TransformOrigin((Length::Percent(0.0), Length::Percent(50.0)))
        );
        assert!(
            matches!(typed("grid-template-columns", "1fr 2fr"), P::GridTemplateColumns(t) if t.len() == 2)
        );
        assert_eq!(
            PropertyDeclaration::parse("border-top-left-radius", "4px"),
            Parsed::Deferred
        );
        assert_eq!(
            PropertyDeclaration::parse("flex-grow", "-1"),
            Parsed::Invalid
        );
        assert_eq!(
            PropertyDeclaration::parse("cursor", "banana"),
            Parsed::Invalid
        );
    }

    #[test]
    fn viewport_math_is_typed_once_the_cascade_knows_the_viewport() {
        assert_eq!(
            PropertyDeclaration::parse("width", "calc(100vw - 20px)"),
            Parsed::Deferred
        );
        assert!(matches!(
            PropertyDeclaration::parse_value("width", "calc(100vw - 20px)"),
            Parsed::Typed(PropertyDeclaration::Width(_))
        ));
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    fn invalid(property: &str, value: &str) -> bool {
        PropertyDeclaration::parse(property, value) == Parsed::Invalid
    }

    #[test]
    fn values_the_spec_rejects_are_invalid() {
        for (property, value) in [
            ("flex-basis", "-50px"),
            ("flex-wrap", "wrap wrap-reverse"),
            ("flex-wrap", "nowrap balance"),
            ("gap", "-1px"),
            ("gap", "1px 2px 3px"),
            ("row-gap", "-5%"),
            ("background-size", "-1px -1px"),
            ("background-size", "1px 2px 3px"),
            ("background-image", "linear-gradient(90degree, red, red)"),
            ("background-image", "linear-gradient(100gradian, red, red)"),
            ("background-image", "linear-gradient(1.57radian, red, red)"),
            ("background-image", "linear-gradient(0.25turns, red, red)"),
            ("background-image", "linear-gradient(red)"),
            ("background-image", "nonsense"),
        ] {
            assert!(invalid(property, value), "{property}: {value}");
        }
        for (property, value) in [
            ("flex-basis", "30px"),
            ("flex-wrap", "wrap-reverse"),
            ("flex-wrap", "balance wrap-reverse"),
            ("align-content", "space-between"),
            ("writing-mode", "sideways-lr"),
            ("gap", "normal 4px"),
            ("background-size", "50% auto"),
            (
                "background-image",
                "linear-gradient(90deg, red, blue 50%, 75%, green)",
            ),
            ("background-image", "linear-gradient(to right, red, blue)"),
            ("background-image", "url(a.png), linear-gradient(red, blue)"),
            ("background-image", "none"),
            ("background-image", "radial-gradient(circle, red, blue)"),
        ] {
            assert!(!invalid(property, value), "{property}: {value}");
        }
        assert_eq!(
            crate::Declaration::new("grid-gap", "20px", false).property,
            "gap"
        );
    }
}
