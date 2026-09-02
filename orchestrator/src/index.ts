import Anthropic from '@anthropic-ai/sdk';
import { buildApp } from './app.js';
import { loadConfig } from './config.js';

const config = loadConfig();
const anthropic = new Anthropic({ apiKey: config.anthropicApiKey });
const app = buildApp({ config, anthropic });

app.listen(config.port, () => {
    console.log(`chat-orchestrator listening on :${config.port}`);
});
