//! Parses Eurostat's SDMX geography codelist. Its per-code annotations say whether a code is still standard
//! and which NUTS level it sits at; the JSON form of the same codelist carries neither.

use std::collections::BTreeMap;
use std::error::Error;

use quick_xml::Reader;
use quick_xml::events::Event;

const ELEMENT_CODE: &str = "Code";
const ELEMENT_ANNOTATION: &str = "Annotation";
const ELEMENT_ANNOTATION_TITLE: &str = "AnnotationTitle";
const ELEMENT_ANNOTATION_TYPE: &str = "AnnotationType";

const ANNOTATION_STANDARD_CODE: &str = "IS_STANDARD_CODE";
const ANNOTATION_LEVEL: &str = "LEVEL";

/// The two `LEVEL` values that name something other than a depth in the territorial hierarchy.
const LEVEL_AGGREGATE: &str = "AGG";
const LEVEL_OTHER: &str = "OTH";

/// Eurostat's judgement on a code, from its `IS_STANDARD_CODE` annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeStanding {
    Standard,
    /// Superseded, and Eurostat intends to stop disseminating it.
    Obsolete,
    /// Carried by the list without belonging to the concept the list codes.
    Unassociated,
}

impl TryFrom<&str> for CodeStanding {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "Y" => Ok(CodeStanding::Standard),
            "O" => Ok(CodeStanding::Obsolete),
            "M" => Ok(CodeStanding::Unassociated),
            unknown => Err(format!("unknown {ANNOTATION_STANDARD_CODE} value; [value={unknown}]")),
        }
    }
}

pub struct GeoCode {
    pub standing: CodeStanding,
    /// Absent for an aggregate, and for a territory the hierarchy does not place.
    pub level: Option<u8>,
}

/// Eurostat gives an aggregate a level of its own rather than a depth, so not every value is a number.
fn parse_level(title: &str) -> Result<Option<u8>, Box<dyn Error>> {
    match title {
        LEVEL_AGGREGATE | LEVEL_OTHER => Ok(None),
        depth => Ok(Some(depth.parse::<u8>()?)),
    }
}

/// Which element's text the reader is inside, an annotation's title and type being the two that carry any.
enum TextSink {
    Title,
    Type,
}

/// One annotation, accumulated across the events between its start and end tags. Eurostat writes the title
/// before the type, so neither can be acted on until the annotation closes.
#[derive(Default)]
struct PendingAnnotation {
    title: String,
    annotation_type: String,
}

#[derive(Default)]
struct PendingCode {
    identifier: String,
    standing: Option<CodeStanding>,
    level: Option<u8>,
}

impl PendingCode {
    fn accept(&mut self, annotation: &PendingAnnotation) -> Result<(), Box<dyn Error>> {
        match annotation.annotation_type.as_str() {
            ANNOTATION_STANDARD_CODE => self.standing = Some(CodeStanding::try_from(annotation.title.as_str())?),
            ANNOTATION_LEVEL => self.level = parse_level(&annotation.title)?,
            _ => {},
        }

        Ok(())
    }

    fn close(self) -> Result<(String, GeoCode), Box<dyn Error>> {
        let standing: CodeStanding = self.standing.ok_or_else(|| {
            format!(
                "a code carries no {ANNOTATION_STANDARD_CODE} annotation; [code={}]",
                self.identifier,
            )
        })?;

        Ok((self.identifier, GeoCode { standing, level: self.level }))
    }
}

pub fn parse_codelist(xml: &str) -> Result<BTreeMap<String, GeoCode>, Box<dyn Error>> {
    let mut reader: Reader<&[u8]> = Reader::from_str(xml);
    let mut geo_codes: BTreeMap<String, GeoCode> = BTreeMap::new();

    let mut open_code: Option<PendingCode> = None;
    let mut annotation: PendingAnnotation = PendingAnnotation::default();
    let mut sink: Option<TextSink> = None;

    loop {
        match reader.read_event()? {
            Event::Start(element) => match element.local_name().into_inner() {
                ELEMENT_CODE => {
                    let identifier = element
                        .try_get_attribute("id")?
                        .ok_or("a Code element carries no id")?;

                    open_code = Some(PendingCode {
                        identifier: identifier.value.to_string(),
                        standing: None,
                        level: None,
                    });
                },
                ELEMENT_ANNOTATION => annotation = PendingAnnotation::default(),
                ELEMENT_ANNOTATION_TITLE => sink = Some(TextSink::Title),
                ELEMENT_ANNOTATION_TYPE => sink = Some(TextSink::Type),
                _ => {},
            },

            Event::Text(text) => match sink {
                Some(TextSink::Title) => annotation.title.push_str(&text.xml10_content()),
                Some(TextSink::Type) => annotation.annotation_type.push_str(&text.xml10_content()),
                None => {},
            },

            Event::End(element) => match element.local_name().into_inner() {
                ELEMENT_ANNOTATION_TITLE | ELEMENT_ANNOTATION_TYPE => sink = None,

                // The codelist annotates itself as well as its codes, so an annotation outside any code is dropped.
                ELEMENT_ANNOTATION => match open_code.as_mut() {
                    Some(pending_code) => pending_code.accept(&annotation)?,
                    None => {},
                },

                ELEMENT_CODE => {
                    let pending_code: PendingCode = open_code
                        .take()
                        .ok_or("a Code element closes unopened")?;
                    let (identifier, geo_code) = pending_code.close()?;

                    geo_codes.insert(identifier, geo_code);
                },

                _ => {},
            },

            Event::Eof => break,

            _ => {},
        }
    }

    Ok(geo_codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODELIST: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<m:Structure xmlns:m="urn:message" xmlns:s="urn:structure" xmlns:c="urn:common">
  <s:Codelist id="GEO">
    <c:Annotations>
      <c:Annotation><c:AnnotationTitle>C</c:AnnotationTitle><c:AnnotationType>IS_STANDARD_CODE_LIST</c:AnnotationType></c:Annotation>
    </c:Annotations>
    <s:Code id="UKN0">
      <c:Annotations>
        <c:Annotation><c:AnnotationTitle>O</c:AnnotationTitle><c:AnnotationType>IS_STANDARD_CODE</c:AnnotationType><c:AnnotationText xml:lang="en">Obsolete code</c:AnnotationText></c:Annotation>
        <c:Annotation><c:AnnotationTitle>2</c:AnnotationTitle><c:AnnotationType>LEVEL</c:AnnotationType></c:Annotation>
      </c:Annotations>
      <c:Name xml:lang="en">Northern Ireland (UK) (NUTS 2021)</c:Name>
    </s:Code>
    <s:Code id="PL21">
      <c:Annotations>
        <c:Annotation><c:AnnotationTitle>Y</c:AnnotationTitle><c:AnnotationType>IS_STANDARD_CODE</c:AnnotationType></c:Annotation>
        <c:Annotation><c:AnnotationTitle>2</c:AnnotationTitle><c:AnnotationType>LEVEL</c:AnnotationType></c:Annotation>
      </c:Annotations>
    </s:Code>
    <s:Code id="EU27_2020">
      <c:Annotations>
        <c:Annotation><c:AnnotationTitle>M</c:AnnotationTitle><c:AnnotationType>IS_STANDARD_CODE</c:AnnotationType></c:Annotation>
        <c:Annotation><c:AnnotationTitle>AGG</c:AnnotationTitle><c:AnnotationType>LEVEL</c:AnnotationType></c:Annotation>
      </c:Annotations>
    </s:Code>
  </s:Codelist>
</m:Structure>"#;

    #[test]
    fn parse_codelist_reads_the_standing_and_level_of_every_code() {
        let geo_codes: BTreeMap<String, GeoCode> = parse_codelist(CODELIST)
            .expect("the codelist parses");

        assert_eq!(geo_codes.len(), 3);
        assert_eq!(geo_codes["UKN0"].standing, CodeStanding::Obsolete);
        assert_eq!(geo_codes["UKN0"].level, Some(2));
        assert_eq!(geo_codes["PL21"].standing, CodeStanding::Standard);
        assert_eq!(geo_codes["PL21"].level, Some(2));
    }

    #[test]
    fn parse_codelist_leaves_an_aggregate_without_a_level() {
        let geo_codes: BTreeMap<String, GeoCode> = parse_codelist(CODELIST)
            .expect("the codelist parses");

        assert_eq!(geo_codes["EU27_2020"].standing, CodeStanding::Unassociated);
        assert_eq!(geo_codes["EU27_2020"].level, None);
    }

    #[test]
    fn parse_codelist_rejects_an_unknown_standing() {
        let xml: String = CODELIST.replace(
            "<c:AnnotationTitle>Y</c:AnnotationTitle>",
            "<c:AnnotationTitle>Q</c:AnnotationTitle>",
        );

        assert!(parse_codelist(&xml).is_err());
    }

    #[test]
    fn parse_codelist_rejects_a_code_the_list_does_not_annotate() {
        let xml: String = CODELIST.replace(
            "<c:Annotation><c:AnnotationTitle>M</c:AnnotationTitle><c:AnnotationType>IS_STANDARD_CODE</c:AnnotationType></c:Annotation>",
            "",
        );

        assert!(parse_codelist(&xml).is_err());
    }
}
