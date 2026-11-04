# DeepSeek Harness Desktop

VaryAI integrates the official DeepSeek Harness desktop application. Install and initialize it separately from the [official download page](https://www.deepseek.com/en/download/), then add **DeepSeek Harness** in **Tools / Agent**. This integration does not install or start the client during detection.

## Shared models and inspection

Use the model dropdown on the DeepSeek Harness row in **Tools / Agent** to connect or change its starting model. VaryAI writes that choice as the default for new Harness sessions. The managed catalog uses OpenAI Chat Completions against the local gateway; the shared protocol bridge handles other configured upstream protocols and official account targets. Model names, declared image support, context capacity and reasoning choices are projected into Harness's native provider configuration. Unknown capacity uses a conservative local fallback, without changing the shared upstream metadata.

The desktop profile is `$DSH_HOME/profiles/desktop/cordis.patch.yml`, with `DSH_HOME` defaulting to `~/.dsh`. The Web profile is separate and is not changed. VaryAI adds its own `@deepseek-ai/dsh-llm-pi-ai` plugin entries and a new-session default model override in a marked block. Existing plugin patches outside this block remain unchanged during shared-catalog changes. An empty YAML list becomes a sequence when providers are added. Disconnecting restores the owned fields while retaining unrelated edits; edits to owned fields require recovery instead of silent replacement. Invalid patch documents or unsupported YAML expressions stop configuration changes.

Existing conversations retain their own model selection; switch those inside Harness, or start a new session to use the default chosen in VaryAI. Disconnecting removes the managed default override and restores the preceding profile. VaryAI leaves the client's official account and credential store with the client. Native DeepSeek account traffic is outside this adapter's inspection coverage; select a VaryAI model to route that conversation through inspection. Existing pi-ai provider patches with explicit endpoints can use the native API-key routing path. File changes can be reloaded by Harness; if its catalog has not refreshed, quit and reopen it manually.

## Versions and updates

On macOS, presence and installed versions are read from the official `DeepSeek Harness.app` bundle with identity `com.deepseek.dsh`. The homepage includes this agent in individual and batch update checks. Apple Silicon builds using the installed official Nightly feed can use the existing verified desktop updater. An absent, different or unreadable feed does not offer an automatic update; use Harness's own updater or the official download page. Update checks do not download or replace an application.

## Verification boundaries

Automated coverage uses synthetic profile files, temporary storage and loopback fixtures. It covers reversible projections, foreign edits, readable model labels, reasoning and image declarations, the three native API protocols, shared protocol conversion, update-feed trust and homepage update controls. Reading an installed app's public configuration schema or release feed does not establish live chat, signing or installer acceptance.
