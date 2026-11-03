# aaPanel's business model: research for VaryAI, bos-api and bos-agent

> Historical research compiled 2026-10-23. Not an approved price list, commercial plan, delivery inventory, security certification or legal advice. Public pages and main-branch licenses can change. The later [commercialization proposal](../marketing/varyai-bos-api-commercialization-proposal.md) prioritizes personal/local VaryAI and independently useful self-hosted bos-api; that approved positioning supersedes the enterprise/three-layer _exploration_ below, but does not verify any sale or implementation.

## Conclusion and limits of the evidence

aaPanel's published structure offers a useful question for AI products: can a complete free self-hosted baseline lead to optional paid value in protection, management, efficiency and ongoing maintenance? Its public feature/price pages do **not** prove user conversion, revenue, retention, security effectiveness or the economics of three AI products. It also offers a lifetime license, so subscriptions alone cannot explain its commercial model. Choose a real customer and validate the cost of solving their problem before setting packages or prices.

The source research read aaPanel's official website, pricing, quick start, GitHub license, the original CVE record, and adjacent sub2api/oh-my-pi repositories. It did not install or purchase aaPanel, test tenant isolation, performance or recovery, interview customers, obtain revenue/active-user/conversion/renewal data, or inspect working bos-api or bos-agent repositories. Public pages were not preserved as immutable snapshots. Distinguish (1) vendor statements and original vulnerability records, (2) VaryAI repository code paths, (3) project intentions supplied without repository proof, and (4) analysis needing pilot evidence. A code path does not prove shipped behavior.

## aaPanel: product, pricing, licensing and security

aaPanel is a Linux web/server management panel for sites, runtimes, databases, files, certificates and operations, not a provider of managed servers. Public materials describe WordPress, Docker/Compose, backups/monitoring, basic firewall, optional WAF/tamper protection, mail, subaccounts and hosting integrations. A backup UI is not verified disaster recovery; an advertised AI Assistant does not establish where inference runs or what data is transmitted. The public cPanel migration claim specifically mentions WordPress sites and SSL certificates, not all accounts, mail and permissions. [S1][S2][S4][S5]

The About page names AAPANEL LIMITED, Hong Kong, incorporated 2023-08-24, and claims 200+ releases since 2017 and 3 million+ installations. The pricing page separately mentions 5000+ Pro users, 200,000+ users and 3.6 million+ users. All are differently defined vendor claims: do not divide them to calculate conversion or estimate sales. An aaPanel forum search index links the product with BAOTA's international edition, but the source research could not load the article body; no license or version interchangeability follows. [S2][S3][S7]

| Published offer at research time | Stated regular USD price | Qualification                                                                                                                                           |
| -------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Free                             | $0                       | Unlimited website creation and basic website/database/FTP/Docker/file/WordPress management are advertised; compute and administration still cost money. |
| Pro monthly                      | $28.80/month             | Advanced protection, batch/team/analytics and support claims; verify checkout and exact features.                                                       |
| Pro yearly                       | $198/year                | Verify current checkout and terms.                                                                                                                      |
| Pro lifetime                     | $699 once                | A separate $399 anniversary promotion appeared; do not confuse it with regular price or a promise about perpetual operating costs.                      |

The pricing FAQ says a license applies to one server/associated IP with normally ten IP changes, a 14-day Pro trial and 15-day refund guarantee. It says expired subscriptions leave existing Pro features running but prohibit editing/configuring/stopping/reinstalling until renewal. These are **published terms, not experimentally verified**; freezing a security configuration would be especially risky to copy. Unlimited team members does not mean unlimited servers. [S2]

The quick start lists a 1 CPU, 512 MB RAM, 1 GB storage installation minimum and Linux systems including Ubuntu, Debian, AlmaLinux and Rocky Linux; the GitHub README expects root on a clean system. This is not a sizing guarantee for hosted workloads. Published install examples include a TLS certificate-check bypass flag: do not execute a privileged remote installer without checking its origin and TLS handling. [S4][S5]

The `AAPANEL Open Source License Agreement` is custom, **not MIT, Apache or GPL**. It allows certain own-project uses and private modifications retaining attribution, restricts public distribution of modified copies, and limits integrated redistribution; paid plugins need not share all source. The license names Dongguan Baita Network Technology Co., Ltd., unlike the About page's company name; check the actual contracting entity before procurement. Free, source-visible, modifiable and redistributable are distinct rights. [S3][S6]

The original record for **CVE-2026-113506**, published 2026-10-20, describes an authenticated OS command-injection issue in aaPanel's file manager, lists affected versions through 8.25.0 and a CNA CVSS v3.1 score of 9.9. The record read for the source research did **not** state a fixed version. Do not infer that all deployments remain affected or that versions above 8.25.0 are fixed. Check the live record before publication. Security functionality can be paid value, but product patching, permission control and incident response are continuing costs. [S8]

## What might transfer—and what must not

| Observation                                           | Testable interpretation                                  | Missing proof                                                |
| ----------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------ |
| Useful free self-hosted baseline                      | Lowers software trial cost without forcing a short trial | Real retention and total hosting/learning cost               |
| Pro emphasizes security, batch work, team and support | Operational complexity may create willingness to pay     | Who buys, what work is saved and actual conversion           |
| Subscription and lifetime offers coexist              | Continuing improvements might sustain recurring value    | Renewal, support margins and lifetime-service sustainability |

A free version should provide independent value and security fixes, not intentionally weakened baseline safety. Offer explicit exit/export and explain expiry terms. Do not blindly copy per-IP licensing onto elastic gateways, lifetime fees onto recurring model-inference costs, or a feature list into a claim of enterprise reliability. Central logs can become sensitive datasets; self-hosting does not prevent a model-provider transmission. [cPanel][S13] serves websites/hosts, [CloudPanel][S15] is a free web panel and [Coolify][S14] organizes Git-driven self-hosted deployments. These are positioning references, **not** a safety or maturity ranking. [sub2api][S10] and [oh-my-pi][S9] are adjacent gateway and agent references; upstream features do not prove downstream delivery.

## Three independent projects, not a proven suite

| Project   | Evidence available here                                                                                                                                                                                                                                                   | What remains unverified                                                                                                                                          |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| VaryAI    | This repository contains [local DLP](../architecture/security-audit.md), [inspection entry](../../src-tauri/src/proxy/security/mod.rs), [tool observations](../architecture/tool-risk.md) and [warning-only risk types](../../src-tauri/src/services/tool_risk/types.rs). | Full release/client coverage or organization-enforced policy. Only routed, supported requests are checked; tool observation is not tool-execution approval.      |
| bos-api   | Described as a gateway with possible easy deployment, operations, metering and security; sub2api is a reference.                                                                                                                                                          | Actual repository, completed features, tenancy, metering correctness, supplier authorization, users, performance, revenue or paid services.                      |
| bos-agent | Described as an agent harness with an upstream sometimes named “on-my-pi”; oh-my-pi is only a conditional comparison.                                                                                                                                                     | Actual upstream identity, fork version, delivered agent, approvals/isolation, pricing or customers. An agent framework is not automatically an execution policy. |

The source research considered _possible_ personal access (VaryAI), concentrated gateway (bos-api) and task execution (bos-agent), potentially sharing identity, policy and event correlation. This is not an approved architecture or a requirement to chain all three products. The later [commercialization proposal](../marketing/varyai-bos-api-commercialization-proposal.md) instead treats VaryAI and bos-api as independently useful products, bos-agent as separate exploration, and personal/local plus small-team self-hosting as the default direction. Enterprise internal AI and external API-service operations are different markets with different purchasing, supplier-license, accounting and liability needs; neither is validated here.

| Hypothesis                                             | Needed evidence                                                          | Possible failure                                           |
| ------------------------------------------------------ | ------------------------------------------------------------------------ | ---------------------------------------------------------- |
| Personal adoption generates organization opportunities | Referrals, administrator engagement, pilot purchases                     | Procurement already standardizes on another platform.      |
| Central management is worth paying for                 | Real policy, usage and cost problems plus a budget owner                 | Existing tools suffice.                                    |
| Controlled agent execution adds value                  | Repeated jobs, explicit permissions, independently checked results       | Existing agents suffice or isolation costs too much.       |
| Cross-product integration helps                        | Reduced setup/administration and useful correlated events                | Extra failures, latency, maintenance and privacy concerns. |
| Security and maintenance sustain payment               | Adopted rule updates, fewer false alarms, renewals and support economics | Poor outcomes or excessive service cost.                   |

This repository's [MIT license](../../LICENSE) does not settle other assets or bos-api's license. [oh-my-pi's license][S11] was MIT and [sub2api's license][S12] LGPL-3.0 on the main branches read by the source research; pin the actual version before reuse and check distribution, notices and supplier terms. sub2api's README contains additional warnings about upstream service terms and commercial operations: software-license rights do **not** authorize sharing subscriptions, reselling API service, or using supplier credentials. Do not infer a universal ban or an unrestricted commercial grant without review.

## Questions for a pilot

1. Which individual/small-team customer has a frequent, expensive problem? Separate internal AI use from running an API business; identify user, consenter and payer.
2. Which bos-api and bos-agent repository versions, capabilities and rights can be demonstrated rather than assumed from upstream projects?
3. Which free baseline and exit rights must remain, and which _additional_ work—protection review, operational diagnosis or support—actually merits paid maintenance?
4. What are observed outcomes, false-positive coverage, latency, model/hosting/support costs, purchase intent and renewal reasons? Do not substitute download counts or a vendor website's user claims.

**Bottom line:** borrow the question “complete base value → incremental management value → sustainable continuing service,” not aaPanel's price list or an unverified claim that these AI products already constitute a profitable suite.

## Sources from the original research (recheck live versions before use)

[S1]: https://www.aapanel.com/
[S2]: https://www.aapanel.com/new/pricing.html
[S3]: https://www.aapanel.com/new/about.html
[S4]: https://www.aapanel.com/docs/guide/quickstart.html
[S5]: https://github.com/aaPanel/aaPanel
[S6]: https://raw.githubusercontent.com/aaPanel/aaPanel/master/license.txt
[S7]: https://www.aapanel.com/forum/d/7-who-is-aapanel
[S8]: https://cveawg.mitre.org/api/cve/CVE-2026-113506
[S9]: https://github.com/can1357/oh-my-pi
[S10]: https://github.com/Wei-Shaw/sub2api
[S11]: https://raw.githubusercontent.com/can1357/oh-my-pi/main/LICENSE
[S12]: https://raw.githubusercontent.com/Wei-Shaw/sub2api/main/LICENSE
[S13]: https://www.cpanel.net/
[S14]: https://coolify.io/
[S15]: https://www.cloudpanel.io/
