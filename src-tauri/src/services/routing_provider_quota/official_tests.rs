use super::{official::Source, *};
use serde_json::json;

#[test]
fn selects_by_official_https_origin_and_keeps_real_zero_wallets() -> Result<(), String> {
    for base in [
        "https://api.deepseek.com.example/v1",
        "http://api.deepseek.com/v1",
        "https://api.deepseek.com:8443/v1",
        "https://relay.example/api.deepseek.com",
        "https://example@api.deepseek.com",
    ] {
        assert!(Source::resolve(base, false).is_none());
    }
    let source = Source::resolve("https://api.deepseek.com/anthropic/v1/messages", false)
        .ok_or("missing source")?;
    assert_eq!(source.url.as_str(), "https://api.deepseek.com/user/balance");
    let q = source.parse(&json!({"is_available":false,"balance_infos":[{"currency":"CNY","total_balance":"0.00"},{"currency":"USD","total_balance":"1.25"},{"currency":"USD"},{"currency":"bad-unit","total_balance":4}]}));
    assert_eq!(q.status, Status::Ready);
    assert_eq!(q.balances.len(), 2);
    assert!(q
        .balances
        .iter()
        .any(|w| w.unit == "CNY" && w.amount == 0.0));
    assert_eq!(source.parse(&json!({})).status, Status::Unsupported);
    Ok(())
}

#[test]
fn official_wallets_preserve_units_and_require_spendable_amounts() -> Result<(), String> {
    for (base, path, body, unit, amount) in [
        (
            "https://api.moonshot.cn/v1",
            "/v1/users/me/balance",
            json!({"data":{"available_balance":"2.25"}}),
            "CNY",
            2.25,
        ),
        (
            "https://api.moonshot.ai/v1",
            "/v1/users/me/balance",
            json!({"data":{"available_balance":0}}),
            "USD",
            0.0,
        ),
        (
            "https://api.stepfun.ai/v1",
            "/v1/accounts",
            json!({"balance":2}),
            "USD",
            2.0,
        ),
        (
            "https://api.stepfun.com/v1",
            "/v1/accounts",
            json!({"balance":3}),
            "CNY",
            3.0,
        ),
        (
            "https://openrouter.ai/api/v1",
            "/api/v1/credits",
            json!({"data":{"total_credits":9,"total_usage":3}}),
            "USD",
            6.0,
        ),
        (
            "https://aihubmix.com/v1",
            "/dashboard/billing/remain",
            json!({"total_usage":0}),
            "USD",
            0.0,
        ),
        (
            "https://api-inference.modelscope.cn/v1",
            "/openapi/v1/magicubes/balance",
            json!({"data":{"total_balance":100,"available_balance":80}}),
            "magicubes",
            80.0,
        ),
        (
            "https://open.bigmodel.cn/api/paas/v4",
            "/api/biz/account/query-customer-account-report",
            json!({"data":{"availableBalance":0}}),
            "CNY",
            0.0,
        ),
        (
            "https://api.z.ai/api/paas/v4",
            "/api/biz/account/query-customer-account-report",
            json!({"data":{"availableBalance":5}}),
            "USD",
            5.0,
        ),
        (
            "https://api.novita.ai/v3",
            "/v3/user/balance",
            json!({"availableBalance":25000}),
            "USD",
            2.5,
        ),
    ] {
        let source = Source::resolve(base, false).ok_or("missing source")?;
        assert_eq!(source.url.path(), path);
        let q = source.parse(&body);
        assert_eq!(q.status, Status::Ready);
        assert_eq!(q.balances[0].unit, unit);
        assert_eq!(q.balances[0].amount, amount);
        assert_eq!(source.parse(&json!({})).status, Status::Unsupported);
    }
    for (base, body) in [
        (
            "https://openrouter.ai",
            json!({"data":{"total_credits":30}}),
        ),
        ("https://aihubmix.com", json!({"total_usage":-1})),
        ("https://api.siliconflow.cn", json!({"code":20092})),
        ("https://opencode.ai/zen/v1", json!({})),
        (
            "https://api.moonshot.cn",
            json!({"status":false,"data":{"available_balance":3}}),
        ),
    ] {
        assert_eq!(
            Source::resolve(base, false)
                .ok_or("source")?
                .parse(&body)
                .status,
            Status::Unsupported
        );
    }
    Ok(())
}

#[test]
fn reads_account_and_plan_windows_only_when_reported() -> Result<(), String> {
    for (base, coding, body, expected) in [
        (
            "https://api.z.ai/api/anthropic",
            true,
            json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":0},{"type":"TOKENS_LIMIT","unit":6}]}}),
            0.0,
        ),
        (
            "https://api.minimax.cn/v1",
            false,
            json!({"base_resp":{"status_code":0},"model_remains":[{"model_name":"general","current_interval_remaining_percent":40}]}),
            60.0,
        ),
        (
            "https://opencode.ai/inference/go/v1",
            false,
            json!({"usage":{"rolling":{"percent":75}}}),
            75.0,
        ),
        (
            "https://api.kimi.ai/coding/v1",
            false,
            json!({"usages":{"limitMonthTotal":{"usedRatio":0.85}}}),
            85.0,
        ),
        (
            "https://api.kimi.com/coding",
            false,
            json!({"usage":{"limit":"20","remaining":"10"}}),
            50.0,
        ),
        (
            "https://ollama.com/v1",
            false,
            json!({"purchased":{"balance_usd":18},"included":{"weekly":{"remaining_percent":20}}}),
            80.0,
        ),
        (
            "https://api.commandcode.ai/v1",
            false,
            json!({"credits":{"planId":"individual-go","monthlyCredits":8,"freeCredits":2},"windowLimits":{"fiveHour":{"used":4,"cap":8}}}),
            50.0,
        ),
    ] {
        let source = Source::resolve(base, coding).ok_or("missing source")?;
        let q = source.parse(&body);
        assert_eq!(q.status, Status::Ready);
        assert_eq!(q.tiers.len(), 1);
        assert_eq!(q.tiers[0].utilization, expected);
        assert_eq!(source.parse(&json!({})).status, Status::Unsupported);
        if base.contains("commandcode") {
            assert_eq!(q.balances[0].unit, "credits");
            assert_eq!(q.balances[1].amount, 2.0);
        }
    }
    Ok(())
}
