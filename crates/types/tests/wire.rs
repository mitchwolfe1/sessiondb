use serde_json::{Value, json};
use sessiondb_types::{ContentBlock, FullContent, ImportManifest, Message};

#[test]
fn golden_message_and_manifest_match_public_types() {
    let message: Message =
        serde_json::from_str(include_str!("../../../docs/examples/message.json")).unwrap();
    assert_eq!(message.content.len(), 2);
    let import: ImportManifest =
        serde_json::from_str(include_str!("../../../docs/examples/import-request.json")).unwrap();
    assert_eq!(import.files.len(), 1);
    let mut invalid = serde_json::to_value(import).unwrap();
    invalid["files"][0]
        .as_object_mut()
        .unwrap()
        .remove("expected_version");
    assert!(serde_json::from_value::<ImportManifest>(invalid).is_err());
}

#[test]
fn future_content_survives_client_round_trips() {
    let block = json!({"type":"future_multimodal","payload":{"n":17}});
    let parsed: ContentBlock = serde_json::from_value(block.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), block);
    let full = json!({"message_id":"77777777-7777-4777-8777-777777777777","revision":"future","content":[block]});
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
