use super::*;

#[test]
fn regional_catalog_keeps_cli_and_enterprise_models_only() -> Result<(), String> {
    let fixture = json!({"agents":[{"name":"ide","models":["private"]},{"name":"cli","models":["glm-5.3","glm-5.3","claude-fixture"]}],"models":[
        {"id":"private","apiKey":"synthetic"},
        {"id":"glm-5.3","name":"GLM","supportsImages":true,"maxInputTokens":1000000,"maxOutputTokens":24000},
        {"id":"claude-fixture","reasoning":{"supportedEfforts":["high"]}},
        {"id":"custom:team|team"}, {"id":"custom:bad\n"}
    ]});
    let china = catalog(SubscriptionProvider::Workbuddy, &fixture)?;
    let international = catalog(SubscriptionProvider::WorkbuddyAi, &fixture)?;
    assert_eq!(china.len(), 3);
    assert_eq!(china[0]["inputModalities"], json!(["text", "image"]));
    assert_eq!(international[0]["inputModalities"], json!(["text"]));
    assert_eq!(china[0]["contextWindow"], 1000000);
    assert_eq!(china[0]["outputTokens"], 24000);
    assert_eq!(china[1]["reasoningLevels"], json!(["none", "high", "max"]));
    assert_eq!(international[1]["reasoningLevels"], json!(["none", "high"]));
    assert_eq!(china[2]["model"], "custom:team|team");
    assert!(catalog(SubscriptionProvider::Workbuddy, &json!({"models":[]})).is_err());
    Ok(())
}

#[test]
fn browser_authority_is_bound_to_the_edition() {
    for (edition, host) in [
        (SubscriptionProvider::Workbuddy, "www.workbuddy.cn"),
        (SubscriptionProvider::WorkbuddyAi, "www.workbuddy.ai"),
    ] {
        let auth = json!({"authUrl":format!("https://{host}/authorize?state=synthetic")});
        assert!(browser_url(edition, &auth).is_ok_and(|url| url
            .query_pairs()
            .any(|(k, v)| k == "version" && v == VERSION)));
        for url in [
            format!("https://{host}.invalid/"),
            format!("http://{host}/"),
            format!("https://fixture@{host}/"),
        ] {
            assert!(browser_url(edition, &json!({"authUrl":url})).is_err());
        }
    }
}
