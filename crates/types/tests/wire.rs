use serde_json::{Value, json};
use sessiondb_types::{ContentBlock, FileUpload, FullContent};

#[test]
fn file_upload_requires_explicit_expected_version() {
    let upload: FileUpload =
        serde_json::from_str(include_str!("../../../docs/examples/file-upload.json")).unwrap();
    let mut invalid = serde_json::to_value(upload).unwrap();
    invalid.as_object_mut().unwrap().remove("expected_version");
    assert!(serde_json::from_value::<FileUpload>(invalid).is_err());
}

#[test]
fn future_content_survives_client_round_trips() {
    let block = json!({"type":"future_multimodal","payload":{"n":17}});
    let parsed: ContentBlock = serde_json::from_value(block.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), block);
    let full = json!({"message_id":"77777777-7777-4777-8777-777777777777","content":[block]});
    let parsed: FullContent = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), full);
    let reference: Value = serde_json::from_str(include_str!(
        "../../../docs/examples/content-reference.json"
    ))
    .unwrap();
    assert!(matches!(
        serde_json::from_value::<ContentBlock>(reference).unwrap(),
        ContentBlock::Reference(_)
    ));
}

#[test]
fn team_filters_are_optional_and_handles_are_canonical() {
    use sessiondb_types::{MemberHandle, SearchFilters, SourceCreate};
    let filters = serde_json::to_value(SearchFilters::default()).unwrap();
    assert!(filters.get("member").is_none() && filters.get("device_label").is_none());
    let mut source = json!({"harness":"pi","device_label":"desk","root":"/fixture/pi"});
    let old: SourceCreate = serde_json::from_value(source.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), source);
    source["member"] = json!("Ada-2");
    let attributed: SourceCreate = serde_json::from_value(source).unwrap();
    assert_eq!(attributed.member.unwrap().as_str(), "ada-2");
    for invalid in ["", "-x", "Mary Jane", "tab\t", "é", &"x".repeat(65)] {
        assert!(invalid.parse::<MemberHandle>().is_err());
    }
}
