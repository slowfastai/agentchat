# Pi Agent Configuration

Pi uses two configuration files under `~/.pi/agent/`:

## `models.json`

Defines available AI providers — their API endpoint, key, and model list. Each top-level key is a provider name:

```json
{
  "providers": {
    "rightcode": {
      "baseUrl": "https://right.codes/codex/v1",
      "api": "openai-responses",
      "apiKey": "sk-...",
      "models": [
        { "id": "gpt-5.4", "name": "GPT-5.4", ... }
      ]
    }
  }
}
```

To switch providers, replace `models.json` with a different config file (e.g. rename `models.json.codex-for-me` to `models.json`).

## `settings.json`

Controls Pi's runtime behaviour. Key fields:

| Field | Description |
|---|---|
| `defaultProvider` | Which provider key from `models.json` to use |
| `defaultModel` | Default model ID within that provider |
| `defaultThinkingLevel` | Reasoning depth: `medium`, `high`, `xhigh` |
| `quietStartup` | Set `true` to suppress skill/command listing on session start |
| `packages` | Installed extension packages (e.g. `pi-autoresearch`) |

## Switching Providers

`defaultProvider` in `settings.json` **must match** a key in `models.json`. If they are out of sync, Pi silently fails with `fetch failed`.

When you swap `models.json` for a different provider config, update `settings.json` to match:

```json
{
  "defaultProvider": "rightcode",
  "defaultModel": "gpt-5.4"
}
```

## Recommended Settings for AgentChat

```json
{
  "quietStartup": true
}
```

Without `quietStartup: true`, Pi emits its full skill/command list at the start of every session, which floods the AgentChat thread feed.
