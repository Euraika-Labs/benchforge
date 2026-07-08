import { invoke } from '@tauri-apps/api/core';
import type { Adapter, AddedBenchmarkPackPromptTask, Artifact, BenchmarkPack, BenchmarkPackCalibrationSuggestion, BenchmarkPackDiagnostic, BenchmarkPackTask, CloudModel, CreatedBenchmarkPackTemplate, CreateTargetBenchmarkHandoffResult, DeletedBenchmarkPackTask, DiagnosticEventRequest, DiagnosticRecord, DoctorCheck, DownloadedModel, ExportedBenchmarkPack, HarnessToolResult, HuggingFaceDownloadJob, HuggingFaceDownloadPlan, HuggingFaceModel, HuggingFaceModelFiles, HuggingFaceServerJob, HuggingFaceStatus, ImportedBenchmarkPack, InstallToolsResult, LocalRuntime, LocalRuntimeToolResult, ModelPreflight, RedactedTargetExport, RunEstimate, RunJob, RunResult, ScorePromptTaskPreview, Target, TargetValidation, UpdatedBenchmarkPackCalibration, UpdatedBenchmarkPackPromptTask } from './types';

export function isDesktopRuntime() {
  if (typeof window === 'undefined') {
    return false;
  }
  const runtimeWindow = window as typeof window & {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: unknown;
  };
  return Boolean(runtimeWindow.__TAURI_INTERNALS__ || runtimeWindow.__TAURI__);
}

const isTauri = isDesktopRuntime;
const browserProviderKeys = new Set<string>();

export interface ProviderApiKeyStatus {
  provider: string;
  available: boolean;
  source: 'keychain' | 'environment' | 'missing' | string;
  detail: string;
  envVar?: string | null;
}

export async function listTargets(): Promise<Target[]> {
  if (!isTauri()) {
    return [
      { id: 'mock-agent', name: 'Mock Agent', kind: 'mock', adapterId: 'mock', status: 'valid', enabled: true },
      { id: 'benchforge-worker', name: 'BenchForge Worker', kind: 'benchmark_harness', adapterId: 'benchforge-worker', status: 'valid', enabled: true },
      { id: 'qwen-ollama', name: 'Qwen via Ollama', kind: 'direct_model', adapterId: 'ollama-openai', status: 'unknown', enabled: true, isLocalModel: true },
      { id: 'cloud-gpt', name: 'Cloud GPT preview', kind: 'direct_model', adapterId: 'openai', status: 'valid', enabled: true, isCloudModel: true },
      { id: 'codex-cli', name: 'Codex CLI', kind: 'cli_agent', adapterId: 'codex-cli', status: 'unknown', enabled: true },
    ];
  }
  return invoke<Target[]>('list_targets');
}

export async function createTarget(target: { id: string; name: string; kind: string; adapterId: string; config?: Record<string, unknown> }): Promise<Target> {
  if (!isTauri()) {
    return { id: target.id, name: target.name, kind: target.kind as Target['kind'], adapterId: target.adapterId, status: 'valid', enabled: true };
  }
  return invoke<Target>('create_target', { request: target });
}

export async function createTargetWithBenchmarkHandoff(
  target: { id: string; name: string; kind: string; adapterId: string; config?: Record<string, unknown> },
  options: { benchmarkPackId?: string; benchmarkTargetIds?: string[]; repetitions?: number; warmupRuns?: number; concurrency?: number; maxCostUsd?: number } = {},
): Promise<CreateTargetBenchmarkHandoffResult> {
  if (!isTauri()) {
    const saved = await createTarget(target);
    const validation = await validateTarget(saved.id);
    let runJob: RunJob | null = null;
    let benchmarkError: string | null = null;
    if (options.benchmarkPackId && validation.status !== 'error') {
      try {
        const targetIds = options.benchmarkTargetIds?.length ? options.benchmarkTargetIds : [saved.id];
        runJob = await startRunJob(
          targetIds,
          false,
          options.benchmarkPackId,
          options.repetitions ?? 1,
          options.warmupRuns ?? 0,
          options.concurrency ?? 1,
          options.maxCostUsd,
        );
      } catch (error) {
        benchmarkError = String(error);
      }
    }
    return { target: saved, validation, runJob, benchmarkError };
  }
  return invoke<CreateTargetBenchmarkHandoffResult>('create_target_with_benchmark_handoff', {
    request: {
      target,
      benchmarkPackId: options.benchmarkPackId,
      benchmarkTargetIds: options.benchmarkTargetIds,
      repetitions: options.repetitions ?? 1,
      warmupRuns: options.warmupRuns ?? 0,
      concurrency: options.concurrency ?? 1,
      maxCostUsd: options.maxCostUsd,
    },
  });
}

export async function setTargetEnabled(id: string, enabled: boolean): Promise<Target> {
  if (!isTauri()) {
    if (id === 'mock-agent' && !enabled) {
      throw new Error('target mock-agent cannot be disabled');
    }
    return { id, name: id, kind: 'direct_model', adapterId: 'mock', status: enabled ? 'valid' : 'invalid', enabled };
  }
  return invoke<Target>('set_target_enabled', { id, enabled });
}

export async function deleteTarget(id: string): Promise<boolean> {
  if (!isTauri()) {
    return id !== 'mock-agent';
  }
  return invoke<boolean>('delete_target', { id });
}

export async function exportTargetRedacted(id: string): Promise<RedactedTargetExport> {
  if (!isTauri()) {
    const browserTargets: Record<string, RedactedTargetExport> = {
      'mock-agent': {
        id: 'mock-agent',
        name: 'Mock Agent',
        kind: 'mock',
        adapter_id: 'mock',
        config: { mode: 'deterministic-fixture-fix' },
      },
      'benchforge-worker': {
        id: 'benchforge-worker',
        name: 'BenchForge Worker',
        kind: 'benchmark_harness',
        adapter_id: 'benchforge-worker',
        config: { command: 'benchforge-worker' },
      },
      'qwen-ollama': {
        id: 'qwen-ollama',
        name: 'Qwen via Ollama',
        kind: 'direct_model',
        adapter_id: 'ollama-openai',
        config: {
          model: 'qwen2.5-coder:7b',
          base_url: 'http://localhost:11434/v1',
          temperature: 0,
          top_p: 1,
          max_tokens: 512,
          timeout_seconds: 120,
          retry_count: 1,
        },
      },
      'cloud-gpt': {
        id: 'cloud-gpt',
        name: 'Cloud GPT preview',
        kind: 'direct_model',
        adapter_id: 'openai',
        config: {
          model: 'gpt-4.1-mini',
          base_url: 'https://api.openai.com/v1',
          api_key_keychain: '<redacted>',
          temperature: 0,
          top_p: 1,
          max_tokens: 512,
          timeout_seconds: 120,
          retry_count: 1,
          input_price_usd_per_million_tokens: 0.4,
          output_price_usd_per_million_tokens: 1.6,
        },
      },
      'codex-cli': {
        id: 'codex-cli',
        name: 'Codex CLI',
        kind: 'cli_agent',
        adapter_id: 'codex-cli',
        config: { command: 'codex' },
      },
    };
    const target = browserTargets[id];
    if (!target) {
      throw new Error(`target ${id} not found`);
    }
    return target;
  }
  return invoke<RedactedTargetExport>('export_target_redacted', { id });
}

export async function validateTarget(id: string): Promise<TargetValidation> {
  if (!isTauri()) {
    return { targetId: id, status: 'ok', detail: 'browser mock validation', checkedAt: new Date().toISOString() };
  }
  return invoke<TargetValidation>('validate_target', { id });
}

export async function recordDiagnosticEvent(request: DiagnosticEventRequest): Promise<DiagnosticRecord> {
  if (!isTauri()) {
    return {
      id: `browser-${Date.now()}`,
      kind: request.kind,
      level: request.level ?? 'error',
      message: request.message,
      detail: request.detail,
      createdAt: new Date().toISOString(),
      logPath: 'browser-memory',
    };
  }
  return invoke<DiagnosticRecord>('record_diagnostic_event', { request });
}

export async function listDiagnostics(limit = 25): Promise<DiagnosticRecord[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<DiagnosticRecord[]>('list_diagnostics', { request: { limit } });
}

export async function estimateRunPlan(targetIds: string[], benchmarkPackId: string, repetitions: number, warmupRuns: number, concurrency: number, taskIds: string[] = []): Promise<RunEstimate> {
  if (!isTauri()) {
    const targetCount = targetIds.length;
    const taskCount = taskIds.length || (benchmarkPackId === 'quick-smoke' ? 2 : 3);
    const measuredRuns = targetCount * taskCount * Math.max(1, repetitions);
    const warmupCalls = targetCount * Math.max(0, warmupRuns);
    const estimatedMeasuredTimeoutSeconds = measuredRuns * 120;
    const estimatedWarmupTimeoutSeconds = warmupCalls * 120;
    return {
      targetCount,
      taskCount,
      repetitions: Math.max(1, repetitions),
      warmupRuns: Math.max(0, warmupRuns),
      concurrency: Math.max(1, Math.min(8, concurrency)),
      measuredRuns,
      warmupCalls,
      totalModelCalls: measuredRuns + warmupCalls,
      estimatedPromptTokens: measuredRuns * 120,
      estimatedMaxCompletionTokens: (measuredRuns + warmupCalls) * 512,
      estimatedMaxCostUsd: null,
      estimatedMeasuredTimeoutSeconds,
      estimatedWarmupTimeoutSeconds,
      estimatedWallClockTimeoutSeconds: estimatedWarmupTimeoutSeconds + Math.ceil(estimatedMeasuredTimeoutSeconds / Math.max(1, Math.min(8, concurrency))),
      pricedTargets: 0,
      unpricedTargets: targetIds,
      heavy: false,
      notes: ['Browser estimate uses mock task and pricing data.'],
    };
  }
  return invoke<RunEstimate>('estimate_run_plan', { request: { targetIds, benchmarkPackId, taskIds, repetitions, warmupRuns, concurrency } });
}

export async function detectLocalRuntimes(): Promise<LocalRuntime[]> {
  if (!isTauri()) {
    const detectedAt = new Date().toISOString();
    return [
      {
        id: 'ollama',
        name: 'Ollama',
        adapterId: 'ollama-openai',
        baseUrl: 'http://localhost:11434/v1',
        status: 'ok',
        detail: 'browser mock: 1 model available',
        probeUrl: 'http://localhost:11434/v1/models',
        modelSource: 'openai_models',
        detectedAt,
        models: ['qwen2.5-coder:7b'],
        recommendedModel: 'qwen2.5-coder:7b',
        installCommand: 'brew install ollama',
        startCommand: 'ollama serve; ollama pull qwen2.5-coder:7b',
        modelHint: 'qwen2.5-coder:7b',
        setupHint: 'Start Ollama and pull at least one model before benchmarking.',
      },
      {
        id: 'lm-studio',
        name: 'LM Studio',
        adapterId: 'lm-studio-openai',
        baseUrl: 'http://localhost:1234/v1',
        status: 'error',
        detail: 'browser mock: not detected',
        probeUrl: null,
        modelSource: null,
        detectedAt,
        models: [],
        recommendedModel: null,
        installCommand: 'Install LM Studio from https://lmstudio.ai',
        startCommand: 'Enable Developer > Local Server in LM Studio',
        modelHint: 'loaded model id from LM Studio',
        setupHint: 'Load a model in LM Studio and start its OpenAI-compatible local server.',
      },
      {
        id: 'llama-cpp',
        name: 'llama.cpp',
        adapterId: 'llama-cpp-openai',
        baseUrl: 'http://localhost:8080/v1',
        status: 'error',
        detail: 'browser mock: not detected',
        probeUrl: null,
        modelSource: null,
        detectedAt,
        models: [],
        recommendedModel: null,
        installCommand: 'brew install llama.cpp',
        startCommand: 'llama-server -m /path/to/model.gguf --host 127.0.0.1 --port 8080',
        modelHint: 'served GGUF model id',
        setupHint: 'Use Settings > Hugging Face Local Model for a guided GGUF download and llama-server start.',
      },
      {
        id: 'vllm',
        name: 'vLLM',
        adapterId: 'vllm-openai',
        baseUrl: 'http://localhost:8000/v1',
        status: 'error',
        detail: 'browser mock: not detected',
        probeUrl: null,
        modelSource: null,
        detectedAt,
        models: [],
        recommendedModel: null,
        installCommand: 'python3 -m pip install vllm',
        startCommand: 'python3 -m vllm.entrypoints.openai.api_server --model Qwen/Qwen2.5-7B-Instruct',
        modelHint: 'Qwen/Qwen2.5-7B-Instruct',
        setupHint: 'Start vLLM with an OpenAI-compatible API server.',
      },
      {
        id: 'mlx-lm',
        name: 'MLX / mlx-lm',
        adapterId: 'mlx-lm',
        baseUrl: 'http://localhost:8080/v1',
        status: 'error',
        detail: 'browser mock: not detected',
        probeUrl: null,
        modelSource: null,
        detectedAt,
        models: [],
        recommendedModel: null,
        installCommand: 'python3 -m pip install mlx-lm',
        startCommand: 'mlx_lm.server --model mlx-community/Qwen2.5-7B-Instruct-4bit --host 127.0.0.1 --port 8080',
        modelHint: 'mlx-community/Qwen2.5-7B-Instruct-4bit',
        setupHint: 'Start mlx-lm with an MLX model.',
      },
      {
        id: 'omlx',
        name: 'oMLX experimental',
        adapterId: 'omlx-experimental',
        baseUrl: 'http://localhost:11435/v1',
        status: 'error',
        detail: 'browser mock: not detected',
        probeUrl: null,
        modelSource: null,
        detectedAt,
        models: [],
        recommendedModel: null,
        installCommand: 'Install oMLX from its project documentation',
        startCommand: 'Start oMLX with its OpenAI-compatible server on port 11435',
        modelHint: 'oMLX served model id',
        setupHint: 'Use this for experimental oMLX OpenAI-compatible endpoints.',
      },
    ];
  }
  return invoke<LocalRuntime[]>('detect_local_runtimes');
}

export type LocalRuntimeToolAction = 'install' | 'check' | 'pull';

export async function runLocalRuntimeToolAction(runtimeId: string, action: LocalRuntimeToolAction, model?: string): Promise<LocalRuntimeToolResult> {
  if (!isTauri()) {
    return {
      runtimeId,
      action,
      status: 'unchanged',
      installCommand: 'Desktop app required for local runtime setup.',
      checkCommand: 'Desktop app required for local runtime setup.',
      log: action === 'pull' && model
        ? `Desktop app required to pull ${model} for ${runtimeId}.`
        : 'Desktop app required for local runtime setup.',
    };
  }
  return invoke<LocalRuntimeToolResult>('run_local_runtime_tool_action', { request: { runtimeId, action, model } });
}

export async function searchCloudModels(adapterId: string, query = '', limit = 25, baseUrl?: string, apiKeyKeychain?: string, apiKeyEnv?: string): Promise<CloudModel[]> {
  if (!isTauri()) {
    const browserModels: CloudModel[] = [
      { model: 'gpt-5-mini', name: 'GPT-5 mini', provider: 'OpenAI', inputPriceUsdPerMillionTokens: 0.25, outputPriceUsdPerMillionTokens: 2.0, source: 'browser-preset' },
      { model: 'gpt-4.1-mini', name: 'GPT-4.1 mini', provider: 'OpenAI', inputPriceUsdPerMillionTokens: 0.4, outputPriceUsdPerMillionTokens: 1.6, source: 'browser-preset' },
      { model: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'Anthropic', inputPriceUsdPerMillionTokens: 3.0, outputPriceUsdPerMillionTokens: 15.0, source: 'browser-preset' },
      { model: 'mistral-small-latest', name: 'Mistral Small', provider: 'Mistral', inputPriceUsdPerMillionTokens: 0.1, outputPriceUsdPerMillionTokens: 0.3, source: 'browser-preset' },
      { model: 'mistralai/mistral-small-3.2-24b-instruct', name: 'Mistral Small 3.2 24B', provider: 'OpenRouter', inputPriceUsdPerMillionTokens: 0.1, outputPriceUsdPerMillionTokens: 0.3, contextLength: 128000, source: 'browser-preset' },
      { model: 'gpt-5-mini', name: 'Azure GPT-5 mini deployment', provider: 'Azure OpenAI', source: 'browser-preset', detail: 'Use your Azure deployment name when it differs from the base model.' },
      { model: 'gemini-3.5-flash', name: 'Gemini 3.5 Flash', provider: 'Google Gemini', inputPriceUsdPerMillionTokens: 1.5, outputPriceUsdPerMillionTokens: 9.0, source: 'browser-preset' },
      { model: 'gemini-3.1-flash-lite', name: 'Gemini 3.1 Flash-Lite', provider: 'Google Gemini', inputPriceUsdPerMillionTokens: 0.25, outputPriceUsdPerMillionTokens: 1.5, source: 'browser-preset' },
      { model: 'gemini-2.5-flash', name: 'Gemini 2.5 Flash', provider: 'Google Gemini', inputPriceUsdPerMillionTokens: 0.3, outputPriceUsdPerMillionTokens: 2.5, source: 'browser-preset' },
      { model: 'gemini-2.5-flash-lite', name: 'Gemini 2.5 Flash-Lite', provider: 'Google Gemini', inputPriceUsdPerMillionTokens: 0.1, outputPriceUsdPerMillionTokens: 0.4, source: 'browser-preset' },
    ];
    const needle = query.trim().toLowerCase();
    const providerByAdapter: Record<string, string> = {
      openai: 'OpenAI',
      anthropic: 'Anthropic',
      mistral: 'Mistral',
      openrouter: 'OpenRouter',
      'azure-openai': 'Azure OpenAI',
      gemini: 'Google Gemini',
    };
    const provider = providerByAdapter[adapterId];
    return browserModels
      .filter(model => !provider || model.provider === provider)
      .filter(model => !needle || model.model.toLowerCase().includes(needle) || model.name.toLowerCase().includes(needle))
      .slice(0, limit);
  }
  return invoke<CloudModel[]>('search_cloud_models', { request: { adapterId, query, limit, baseUrl, apiKeyKeychain, apiKeyEnv } });
}

export async function saveProviderApiKey(provider: string, apiKey: string): Promise<ProviderApiKeyStatus> {
  if (!isTauri()) {
    if (apiKey.trim()) {
      browserProviderKeys.add(provider);
    }
    const available = browserProviderKeys.has(provider);
    return {
      provider,
      available,
      source: available ? 'keychain' : 'missing',
      detail: available ? 'Browser preview key is stored for this session' : 'Desktop app required for Keychain storage',
      envVar: null,
    };
  }
  return invoke<ProviderApiKeyStatus>('save_provider_api_key', { request: { provider, apiKey } });
}

export async function providerApiKeyStatus(provider: string): Promise<ProviderApiKeyStatus> {
  if (!isTauri()) {
    const available = browserProviderKeys.has(provider);
    return {
      provider,
      available,
      source: available ? 'keychain' : 'missing',
      detail: available ? 'Browser preview key is stored for this session' : 'Desktop app required for Keychain and environment checks',
      envVar: null,
    };
  }
  return invoke<ProviderApiKeyStatus>('provider_api_key_status', { provider });
}

export async function listAdapters(): Promise<Adapter[]> {
  if (!isTauri()) {
    return [
      { id: 'mock', name: 'Mock', kind: 'mock', adapterVersion: 1, schemaVersion: 1, path: 'browser', validationStatus: 'ok', validationDetail: 'browser mock', capabilities: {}, metadata: {} },
      {
        id: 'openai',
        name: 'OpenAI',
        kind: 'openai_responses',
        adapterVersion: 1,
        schemaVersion: 1,
        defaultBaseUrl: 'https://api.openai.com/v1',
        path: 'browser',
        validationStatus: 'warn',
        validationDetail: 'desktop app required for live validation',
        capabilities: {},
        metadata: {
          pricing_verified_at: '2026-07-06',
          model_presets: [
            { label: 'GPT-5 mini', model: 'gpt-5-mini', input_price_usd_per_million_tokens: 0.25, output_price_usd_per_million_tokens: 2.0 },
            { label: 'GPT-4.1 mini', model: 'gpt-4.1-mini', input_price_usd_per_million_tokens: 0.4, output_price_usd_per_million_tokens: 1.6 },
          ],
        },
      },
      {
        id: 'anthropic',
        name: 'Anthropic Claude',
        kind: 'anthropic_messages',
        adapterVersion: 1,
        schemaVersion: 1,
        defaultBaseUrl: 'https://api.anthropic.com',
        path: 'browser',
        validationStatus: 'warn',
        validationDetail: 'desktop app required for live validation',
        capabilities: {},
        metadata: {
          pricing_verified_at: '2026-07-06',
          model_presets: [
            { label: 'Claude Sonnet 5', model: 'claude-sonnet-5', input_price_usd_per_million_tokens: 2.0, output_price_usd_per_million_tokens: 10.0 },
            { label: 'Claude Sonnet 4.6', model: 'claude-sonnet-4-6', input_price_usd_per_million_tokens: 3.0, output_price_usd_per_million_tokens: 15.0 },
          ],
        },
      },
      {
        id: 'openrouter',
        name: 'OpenRouter',
        kind: 'openai_compatible',
        adapterVersion: 1,
        schemaVersion: 1,
        defaultBaseUrl: 'https://openrouter.ai/api/v1',
        path: 'browser',
        validationStatus: 'warn',
        validationDetail: 'desktop app required for live validation',
        capabilities: {},
        metadata: { pricing_note: 'OpenRouter prices vary by routed model; enter manual prices.' },
      },
      {
        id: 'azure-openai',
        name: 'Azure OpenAI',
        kind: 'azure_openai',
        adapterVersion: 1,
        schemaVersion: 1,
        defaultBaseUrl: 'https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1',
        path: 'browser',
        validationStatus: 'warn',
        validationDetail: 'desktop app required for live validation',
        capabilities: {},
        metadata: { setup_note: 'Use the Azure /openai/v1 base URL and put the deployment name in the Model field.' },
      },
      {
        id: 'gemini',
        name: 'Google Gemini',
        kind: 'openai_compatible',
        adapterVersion: 1,
        schemaVersion: 1,
        defaultBaseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
        path: 'browser',
        validationStatus: 'warn',
        validationDetail: 'desktop app required for live validation',
        capabilities: {
          text_generation: true,
          streaming: true,
          tool_calling: true,
          json_mode: true,
          cost_reporting: true,
          token_usage_reporting: true,
        },
        metadata: {
          docs: 'https://ai.google.dev/gemini-api/docs/openai',
          pricing_source: 'https://ai.google.dev/gemini-api/docs/pricing',
          pricing_verified_at: '2026-07-08',
          setup_note: "Uses Google Gemini's OpenAI-compatible endpoint; get an API key from Google AI Studio.",
          model_presets: [
            { label: 'Gemini 3.5 Flash', model: 'gemini-3.5-flash', input_price_usd_per_million_tokens: 1.5, output_price_usd_per_million_tokens: 9.0 },
            { label: 'Gemini 3.1 Flash-Lite', model: 'gemini-3.1-flash-lite', input_price_usd_per_million_tokens: 0.25, output_price_usd_per_million_tokens: 1.5 },
            { label: 'Gemini 2.5 Flash', model: 'gemini-2.5-flash', input_price_usd_per_million_tokens: 0.3, output_price_usd_per_million_tokens: 2.5 },
            { label: 'Gemini 2.5 Flash-Lite', model: 'gemini-2.5-flash-lite', input_price_usd_per_million_tokens: 0.1, output_price_usd_per_million_tokens: 0.4 },
          ],
        },
      },
      { id: 'codex-cli', name: 'OpenAI Codex CLI', kind: 'cli_agent', adapterVersion: 1, schemaVersion: 1, command: 'codex', path: 'browser', validationStatus: 'warn', validationDetail: 'not checked in browser mode', capabilities: {}, metadata: {} },
      { id: 'benchforge-worker', name: 'BenchForge Worker', kind: 'benchmark_harness', adapterVersion: 1, schemaVersion: 1, command: 'benchforge-worker', path: 'browser', validationStatus: 'ok', validationDetail: 'browser mock', capabilities: { benchmark_harness: true, static_analysis: true }, metadata: { setup_note: 'Uses the bundled Python worker in the desktop app.' } },
    ];
  }
  return invoke<Adapter[]>('list_adapters');
}

export async function listBenchmarkPacks(): Promise<BenchmarkPack[]> {
  if (!isTauri()) {
    const packs = [
      { id: 'quick-smoke', name: 'Quick Smoke', version: '0.1.0', description: 'Small repo-patch runner checks.', tags: ['smoke', 'repo_patch', 'fast'], estimatedRuntime: '1-5 minutes', requiresSandbox: true, tasks: 2, heavy: false, taskTypes: ['repo_patch'], languages: ['javascript', 'python'], requiredTools: ['npm', 'python'], scoringMethods: ['jest', 'pytest'], supportedTargetKinds: ['cli_agent', 'direct_model', 'harnessed_model', 'mock'], targetFit: 'Repo/code-edit agents or model edit targets; sandbox recommended' },
      { id: 'llm-connectivity', name: 'LLM Connectivity', version: '0.1.0', description: 'Minimal prompt checks that verify local/cloud model endpoints respond and BenchForge records run artifacts and metrics.', tags: ['prompt', 'llm', 'smoke', 'connectivity', 'local', 'cloud'], estimatedRuntime: '<1 minute', requiresSandbox: false, tasks: 2, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['non-empty response'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-basics', name: 'LLM Basics', version: '0.1.0', description: 'Fast prompt-only checks for local and cloud models.', tags: ['prompt', 'llm', 'fast'], estimatedRuntime: '1-3 minutes', requiresSandbox: false, tasks: 3, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['contains', 'exact', 'json'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-core', name: 'LLM Core', version: '0.1.0', description: 'Broader prompt-only checks.', tags: ['prompt', 'llm', 'fast', 'core'], estimatedRuntime: '3-8 minutes', requiresSandbox: false, tasks: 6, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['contains', 'json'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-practical', name: 'LLM Practical Selection', version: '0.1.0', description: 'Practical prompt-only checks for local and cloud model selection.', tags: ['prompt', 'llm', 'practical', 'selection'], estimatedRuntime: '10-22 minutes', requiresSandbox: false, tasks: 16, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['contains', 'exact JSON arrays', 'json', 'json field contains', 'json fields', 'numeric bounds', 'numeric tolerance', 'regex'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-decision-suite', name: 'LLM Decision Suite', version: '0.1.0', description: 'Operational decision checks for model selection and benchmark interpretation.', tags: ['prompt', 'llm', 'decision', 'comparison'], estimatedRuntime: '10-25 minutes', requiresSandbox: false, tasks: 10, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['exact JSON arrays', 'json', 'json field contains', 'json fields', 'numeric tolerance'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-structured-output', name: 'LLM Structured Output', version: '0.1.0', description: 'Strict JSON, schema repair, nested extraction, and safe structured refusal checks.', tags: ['prompt', 'llm', 'structured-output', 'json'], estimatedRuntime: '5-12 minutes', requiresSandbox: false, tasks: 6, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['exact JSON arrays', 'exact JSON object keys', 'json', 'json fields', 'numeric tolerance', 'ordered JSON arrays'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-grounded-context', name: 'LLM Grounded Context', version: '0.1.0', description: 'Evidence grounding, citation, distractor resistance, contradiction handling, and unsupported-claim refusal checks.', tags: ['prompt', 'llm', 'grounding', 'citations', 'comparison'], estimatedRuntime: '6-15 minutes', requiresSandbox: false, tasks: 6, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['exact JSON arrays', 'json', 'json field contains', 'json fields', 'numeric tolerance'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'llm-reliability', name: 'LLM Reliability', version: '0.1.0', description: 'Reliability checks for ambiguity, instruction hierarchy, context recall, format pressure, consistency, correction discipline, privacy-preserving evaluation, served-model identity, sample-size caution, confidence-interval overlap caution, SLO gating, and rate-limit retry discipline.', tags: ['prompt', 'llm', 'reliability', 'comparison'], estimatedRuntime: '14-30 minutes', requiresSandbox: false, tasks: 12, heavy: false, taskTypes: ['prompt'], languages: [], requiredTools: [], scoringMethods: ['exact JSON arrays', 'json', 'json field contains', 'json fields', 'numeric tolerance'], supportedTargetKinds: ['direct_model', 'harnessed_model', 'mock'], targetFit: 'Local/cloud chat models and OpenAI-compatible runtimes' },
      { id: 'security-defensive', name: 'Defensive secure-code checks', version: '0.1.0', description: 'Defensive-only static checks for generated patches.', tags: ['security', 'defensive', 'static-analysis'], estimatedRuntime: '5-60 minutes', requiresSandbox: true, tasks: 1, heavy: false, taskTypes: ['benchmark_harness'], languages: ['multi'], requiredTools: ['BenchForge worker'], scoringMethods: ['semgrep', 'fallback static scan'], supportedTargetKinds: ['benchmark_harness'], targetFit: 'External worker-backed harness; check required tools before running' },
      { id: 'evalplus', name: 'EvalPlus', version: '0.1.0', description: 'Worker-backed code-generation harness.', tags: ['evalplus', 'code_generation'], estimatedRuntime: 'varies', requiresSandbox: true, tasks: 2, heavy: false, taskTypes: ['benchmark_harness'], languages: ['python'], requiredTools: ['BenchForge worker'], scoringMethods: ['evalplus'], supportedTargetKinds: ['benchmark_harness'], targetFit: 'External worker-backed harness; check required tools before running' },
      { id: 'aider-polyglot-subset', name: 'Aider Polyglot subset', version: '0.1.0', description: 'Worker-backed file-editing harness subset.', tags: ['aider', 'polyglot', 'file_editing'], estimatedRuntime: '30-180 minutes depending target', requiresSandbox: true, tasks: 1, heavy: true, taskTypes: ['benchmark_harness'], languages: ['multi'], requiredTools: ['BenchForge worker'], scoringMethods: ['aider'], supportedTargetKinds: ['benchmark_harness'], targetFit: 'External worker-backed harness; check required tools before running' },
      { id: 'terminal-bench-subset', name: 'Terminal-Bench subset', version: '0.1.0', description: 'Terminal-agent evaluation subset.', tags: ['terminal-bench', 'cli_agent', 'terminal'], estimatedRuntime: '30-180 minutes depending target', requiresSandbox: true, tasks: 1, heavy: true, taskTypes: ['benchmark_harness'], languages: ['multi'], requiredTools: ['BenchForge worker'], scoringMethods: ['terminal-bench'], supportedTargetKinds: ['benchmark_harness'], targetFit: 'External worker-backed harness; check required tools before running' },
      { id: 'swebench-lite-subset', name: 'SWE-bench Lite subset', version: '0.1.0', description: 'Worker-backed SWE-bench Lite subset.', tags: ['swebench', 'repo_patch', 'python'], estimatedRuntime: '1-6 hours depending target', requiresSandbox: true, tasks: 1, heavy: true, taskTypes: ['benchmark_harness'], languages: ['python'], requiredTools: ['BenchForge worker'], scoringMethods: ['swebench'], supportedTargetKinds: ['benchmark_harness'], targetFit: 'External worker-backed harness; check required tools before running' },
    ];
    return packs.map(pack => {
      const evidence = sampleBenchmarkPackEvidence(pack);
      const calibration = sampleBenchmarkPackCalibration(pack);
      return {
        ...pack,
        promptTasks: pack.taskTypes.includes('prompt') ? pack.tasks : 0,
        totalTaskWeight: pack.tasks,
        evidenceProfile: evidence.profile,
        evidenceWarnings: evidence.warnings,
        calibrationStatus: calibration.status,
        calibrationSampleSize: calibration.sampleSize,
        calibrationBaselineModels: calibration.baselineModels,
        calibrationLastReviewed: calibration.lastReviewed,
        calibrationReviewScope: calibration.reviewScope,
        calibrationQualityGates: calibration.qualityGates,
        calibrationNotes: calibration.notes,
        source: 'browser-sample',
        sourcePath: 'browser preview',
      };
    });
  }
  return invoke<BenchmarkPack[]>('list_benchmark_packs');
}

function sampleBenchmarkPackEvidence(pack: { id: string; tags: string[]; taskTypes: string[]; tasks: number; scoringMethods: string[]; requiresSandbox: boolean }) {
  const allPrompt = pack.taskTypes.length === 1 && pack.taskTypes[0] === 'prompt';
  const connectivity = pack.id.includes('connectivity') || pack.tags.includes('connectivity');
  const smoke = pack.tags.includes('smoke');
  const weakScoring = pack.scoringMethods.length > 0 && pack.scoringMethods.every(method => method === 'non-empty response');
  if (pack.taskTypes.includes('benchmark_harness')) {
    return { profile: 'worker_harness', warnings: pack.requiresSandbox ? [] : ['Worker-backed harness packs should declare sandbox requirements.'] };
  }
  if (allPrompt && connectivity) {
    return { profile: 'connectivity_smoke', warnings: ['Connectivity smoke confirms endpoint response; use a broader prompt pack before model selection.'] };
  }
  if (allPrompt && pack.tasks < 3) {
    return { profile: 'prompt_smoke', warnings: ['Fewer than 3 prompt tasks; run a broader pack before choosing between models.'] };
  }
  if (allPrompt && weakScoring) {
    return { profile: 'weak_prompt_suite', warnings: ['All prompt tasks use non-empty scoring; add exact, JSON, regex, or numeric checks for reliable comparison.'] };
  }
  if (allPrompt) {
    return { profile: 'prompt_comparison', warnings: [] };
  }
  if (smoke) {
    return { profile: 'code_smoke', warnings: ['Smoke packs verify the runner path; use broader packs for model-selection evidence.'] };
  }
  return { profile: 'code_agent', warnings: [] };
}

function sampleBenchmarkPackCalibration(pack: { id: string; tags: string[]; taskTypes: string[] }) {
  if (pack.id.startsWith('llm-') && !pack.tags.includes('connectivity')) {
    return {
      status: 'pilot',
      sampleSize: 0,
      baselineModels: [],
      lastReviewed: '2026-07-08',
      reviewScope: 'contract_review',
      qualityGates: [
        'local_cloud_baseline_pair',
        'provider_confirmed_model_identity',
        'complete_pack_task_coverage',
        'min_3_repetitions_per_task_target',
        'cost_metrics_for_cloud_targets',
        'single_generation_policy',
        'review_before_public_leaderboard',
      ],
      notes: 'Prompt contracts are reviewed for first-pass model selection; they are not empirical public benchmark calibration.',
    };
  }
  return {
    status: 'uncalibrated',
    sampleSize: null,
    baselineModels: [],
    lastReviewed: null,
    reviewScope: null,
    qualityGates: [],
    notes: null,
  };
}

export async function listBenchmarkPackDiagnostics(): Promise<BenchmarkPackDiagnostic[]> {
  if (!isTauri()) {
    const packs = await listBenchmarkPacks();
    return packs.map(pack => ({
      id: pack.id,
      source: pack.source,
      sourcePath: pack.sourcePath,
      status: 'ok',
      detail: `${pack.tasks} task(s) loaded`,
    }));
  }
  return invoke<BenchmarkPackDiagnostic[]>('list_benchmark_pack_diagnostics');
}

export async function listBenchmarkPackTasks(packId: string): Promise<BenchmarkPackTask[]> {
  if (!isTauri()) {
    const taskIdsByPack: Record<string, string[]> = {
      'llm-basics': ['llm-instruction-following-001', 'llm-json-validity-001', 'llm-summarization-001'],
      'llm-connectivity': ['llm-connectivity-nonempty-001', 'llm-connectivity-short-completion-001'],
    };
    const taskIds = taskIdsByPack[packId] ?? [`${packId}-task-001`];
    return taskIds.map((id, index) => ({
      id,
      name: id.replace(/-/g, ' '),
      taskType: 'prompt',
      language: null,
      fixture: null,
      prompt: index === 0 ? 'Reply with exactly OK.' : 'Return a short benchmark response.',
      timeoutSeconds: 120,
      maxTurns: null,
      weight: 1,
      scoringMethods: index === 0 ? ['exact'] : ['non-empty response'],
      scoring: index === 0 ? { expect_exact: 'OK' } : {},
      sourcePath: 'browser preview',
    }));
  }
  return invoke<BenchmarkPackTask[]>('list_benchmark_pack_tasks', { packId });
}

export async function createBenchmarkPackTemplate(request: {
  id: string;
  name: string;
  description?: string;
  prompt: string;
  expectedResponse: string;
}): Promise<CreatedBenchmarkPackTemplate> {
  if (!isTauri()) {
    throw new Error('desktop app required to create benchmark pack files');
  }
  return invoke<CreatedBenchmarkPackTemplate>('create_benchmark_pack_template', { request });
}

export async function addBenchmarkPackPromptTask(request: {
  packId: string;
  taskId?: string;
  name: string;
  prompt: string;
  scoringMethod: string;
  expectedResponse?: string;
  timeoutSeconds?: number;
  weight?: number;
}): Promise<AddedBenchmarkPackPromptTask> {
  if (!isTauri()) {
    throw new Error('desktop app required to edit benchmark pack files');
  }
  return invoke<AddedBenchmarkPackPromptTask>('add_benchmark_pack_prompt_task', { request });
}

export async function updateBenchmarkPackPromptTask(request: {
  packId: string;
  taskId: string;
  name: string;
  prompt: string;
  scoringMethod: string;
  expectedResponse?: string;
  timeoutSeconds?: number;
  weight?: number;
}): Promise<UpdatedBenchmarkPackPromptTask> {
  if (!isTauri()) {
    throw new Error('desktop app required to edit benchmark pack files');
  }
  return invoke<UpdatedBenchmarkPackPromptTask>('update_benchmark_pack_prompt_task', { request });
}

export async function updateBenchmarkPackCalibration(request: {
  packId: string;
  status: string;
  sampleSize?: number;
  baselineModels?: string[];
  lastReviewed?: string;
  notes?: string;
}): Promise<UpdatedBenchmarkPackCalibration> {
  if (!isTauri()) {
    throw new Error('desktop app required to edit benchmark pack calibration');
  }
  return invoke<UpdatedBenchmarkPackCalibration>('update_benchmark_pack_calibration', { request });
}

export async function suggestBenchmarkPackCalibration(request: {
  packId: string;
}): Promise<BenchmarkPackCalibrationSuggestion> {
  if (!isTauri()) {
    throw new Error('desktop app required to suggest benchmark pack calibration');
  }
  return invoke<BenchmarkPackCalibrationSuggestion>('suggest_benchmark_pack_calibration', { request });
}

export async function scorePromptTaskPreview(request: {
  scoringMethod: string;
  expectedResponse?: string;
  sampleResponse: string;
}): Promise<ScorePromptTaskPreview> {
  if (!isTauri()) {
    throw new Error('desktop app required to preview benchmark scoring');
  }
  return invoke<ScorePromptTaskPreview>('score_prompt_task_preview', { request });
}

export async function deleteBenchmarkPackTask(request: {
  packId: string;
  taskId: string;
}): Promise<DeletedBenchmarkPackTask> {
  if (!isTauri()) {
    throw new Error('desktop app required to edit benchmark pack files');
  }
  return invoke<DeletedBenchmarkPackTask>('delete_benchmark_pack_task', { request });
}

export async function exportBenchmarkPack(request: {
  packId: string;
  destinationDir?: string;
  format?: 'folder' | 'zip';
}): Promise<ExportedBenchmarkPack> {
  if (!isTauri()) {
    throw new Error('desktop app required to export benchmark pack folders or zip archives');
  }
  return invoke<ExportedBenchmarkPack>('export_benchmark_pack', { request });
}

export async function importBenchmarkPack(request: {
  sourcePath: string;
}): Promise<ImportedBenchmarkPack> {
  if (!isTauri()) {
    throw new Error('desktop app required to import benchmark pack folders or zip archives');
  }
  return invoke<ImportedBenchmarkPack>('import_benchmark_pack', { request });
}

export async function runQuickSmoke(targetIds: string[], docker = false, benchmarkPackId = 'quick-smoke', repetitions = 1, warmupRuns = 0, concurrency = 1, maxCostUsd?: number, taskIds: string[] = []): Promise<RunResult[]> {
  if (!isTauri()) {
    const tasksByPack: Record<string, string[]> = {
      'llm-connectivity': ['llm-connectivity-nonempty-001', 'llm-connectivity-short-completion-001'],
      'llm-basics': ['llm-instruction-following-001', 'llm-json-validity-001', 'llm-summarization-001'],
      'llm-core': ['llm-core-classification-001', 'llm-core-extraction-001', 'llm-core-arithmetic-001', 'llm-core-tool-call-001', 'llm-core-boundary-001', 'llm-core-synthesis-001'],
      'llm-practical': ['llm-practical-routing-001', 'llm-practical-json-repair-001', 'llm-practical-cost-math-001', 'llm-practical-safety-boundary-001', 'llm-practical-entity-resolution-001', 'llm-practical-contradiction-001', 'llm-practical-tool-payload-001', 'llm-practical-decision-memo-001', 'llm-practical-evidence-grounding-001', 'llm-practical-pii-redaction-001', 'llm-practical-model-tradeoff-001', 'llm-practical-strict-format-001', 'llm-practical-privacy-aware-routing-001', 'llm-practical-budget-cap-model-mix-001', 'llm-practical-regression-triage-001', 'llm-practical-context-pruning-001'],
      'llm-decision-suite': ['llm-decision-model-ranking-001', 'llm-decision-weighted-score-001', 'llm-decision-abstain-001', 'llm-decision-multilingual-extraction-001', 'llm-decision-instruction-conflict-001', 'llm-decision-error-taxonomy-001', 'llm-decision-date-window-001', 'llm-decision-score-normalization-001', 'llm-decision-deduplicate-incidents-001', 'llm-decision-table-to-json-001'],
      'llm-structured-output': ['llm-structured-schema-extraction-001', 'llm-structured-nested-tool-call-001', 'llm-structured-array-normalization-001', 'llm-structured-schema-repair-001', 'llm-structured-refusal-envelope-001', 'llm-structured-numeric-unit-conversion-001'],
      'llm-grounded-context': ['llm-grounded-needle-citation-001', 'llm-grounded-distractor-filter-001', 'llm-grounded-contradiction-resolution-001', 'llm-grounded-unsupported-claim-001', 'llm-grounded-multi-document-synthesis-001', 'llm-grounded-noisy-table-grounding-001'],
      'llm-reliability': ['llm-reliability-ambiguous-requirements-001', 'llm-reliability-instruction-hierarchy-001', 'llm-reliability-context-recall-001', 'llm-reliability-format-pressure-001', 'llm-reliability-multi-step-consistency-001', 'llm-reliability-correction-discipline-001', 'llm-reliability-sample-size-caution-001', 'llm-reliability-privacy-preserving-eval-001', 'llm-reliability-served-model-identity-001', 'llm-reliability-confidence-interval-overlap-001', 'llm-reliability-latency-cost-slo-001', 'llm-reliability-rate-limit-retry-001'],
      'security-defensive': ['semgrep-basic'],
      'evalplus': ['evalplus-humaneval-plus', 'evalplus-mbpp-plus'],
      'aider-polyglot-subset': ['aider-polyglot-subset'],
      'terminal-bench-subset': ['terminal-bench-subset'],
      'swebench-lite-subset': ['swebench-lite-subset'],
      'quick-smoke': ['python-rate-limit-001', 'js-sanitize-filename-001'],
    };
    const availableTasks = tasksByPack[benchmarkPackId] ?? tasksByPack['quick-smoke'];
    const tasks = taskIds.length ? availableTasks.filter(taskId => taskIds.includes(taskId)) : availableTasks;
    return Array.from({ length: Math.max(1, repetitions) }).flatMap((_, index) =>
      (targetIds.length ? targetIds : ['mock-agent']).flatMap(targetId =>
        tasks.map((taskId, taskIndex) => ({
          id: crypto.randomUUID(),
          run_group_id: 'browser-group',
          targetId,
          benchmarkPackId,
          taskId,
          status: 'passed',
          score: 1,
          wallTimeMs: 120 + taskIndex * 200 + index * 8,
          artifacts: ['stdout.txt', 'stderr.txt', 'diff.patch'],
        }))
      )
    );
  }
  return invoke<RunResult[]>('run_quick_smoke', { request: { targetIds, benchmarkPackId, taskIds, repetitions, warmupRuns, concurrency, docker, maxCostUsd } });
}

export async function startRunJob(targetIds: string[], docker = false, benchmarkPackId = 'quick-smoke', repetitions = 1, warmupRuns = 0, concurrency = 1, maxCostUsd?: number, taskIds: string[] = []): Promise<RunJob> {
  if (!isTauri()) {
    const id = crypto.randomUUID();
    const results = await runQuickSmoke(targetIds, docker, benchmarkPackId, repetitions, warmupRuns, concurrency, maxCostUsd, taskIds);
    return { id, runGroupId: crypto.randomUUID(), benchmarkPackId, status: 'completed', message: `browser preview sample run completed with ${warmupRuns} warmup(s), concurrency ${concurrency}`, startedAt: new Date().toISOString(), finishedAt: new Date().toISOString(), total: results.length, completed: results.length, results, settings: { targetCount: targetIds.length, taskCount: taskIds.length || new Set(results.map(result => result.taskId)).size, repetitions, warmupRuns, concurrency, docker, maxCostUsd } };
  }
  return invoke<RunJob>('start_run_job', { request: { targetIds, benchmarkPackId, taskIds, repetitions, warmupRuns, concurrency, docker, maxCostUsd } });
}

export async function listRunJobs(): Promise<RunJob[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<RunJob[]>('list_run_jobs');
}

export async function getRunJob(id: string): Promise<RunJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<RunJob | null>('get_run_job', { id });
}

export async function cancelRunJob(id: string): Promise<RunJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<RunJob | null>('cancel_run_job', { id });
}

export async function duplicateRunJob(id: string): Promise<RunJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<RunJob | null>('duplicate_run_job', { id });
}

export async function retryRunJob(id: string): Promise<RunJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<RunJob | null>('retry_run_job', { id });
}

export async function clearFinishedRunJobs(): Promise<number> {
  if (!isTauri()) {
    return 0;
  }
  return invoke<number>('clear_finished_run_jobs');
}

export async function runWorkerMock(): Promise<RunResult> {
  if (!isTauri()) {
    return { id: crypto.randomUUID(), targetId: 'benchforge-worker', benchmarkPackId: 'worker-mock', taskId: 'worker-mock', status: 'passed', score: 1, wallTimeMs: 50 };
  }
  return invoke<RunResult>('run_worker_mock');
}

export async function listResults(): Promise<RunResult[]> {
  if (!isTauri()) {
    return sampleBrowserResults();
  }
  return invoke<RunResult[]>('list_results');
}

export async function listArtifacts(runId?: string): Promise<Artifact[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<Artifact[]>('list_artifacts', { runId });
}

export async function readArtifact(path: string): Promise<string> {
  if (!isTauri()) {
    return 'Artifact content is available in the desktop app.';
  }
  return invoke<string>('read_artifact', { path });
}

export async function exportResults(format: 'jsonl' | 'csv' | 'markdown' | 'analysis', runIds?: string[]): Promise<string> {
  if (!isTauri()) {
    return '';
  }
  return invoke<string>('export_results', { format, runIds });
}

export async function exportReportFolder(runIds?: string[]): Promise<string> {
  if (!isTauri()) {
    return 'Desktop app required for report folder export.';
  }
  return invoke<string>('export_report_folder', { runIds });
}

export async function huggingFaceStatus(): Promise<HuggingFaceStatus> {
  if (!isTauri()) {
    return {
      tokenAvailable: false,
      pythonAvailable: true,
      pythonSupported: true,
      pythonVersion: '3.11.0',
      hfCliAvailable: false,
      llamaServerAvailable: false,
      serverRunning: false,
      cacheDir: 'browser/.benchforge/models',
      cacheSizeBytes: 0,
      cacheFreeBytes: null,
      detail: 'Desktop app required',
      models: [],
    };
  }
  return invoke<HuggingFaceStatus>('huggingface_status');
}

export async function saveHuggingFaceToken(token: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  return invoke<void>('save_huggingface_token', { request: { token } });
}

export async function installHuggingFaceTools(installHf = true, installLlama = true, installPython = false): Promise<InstallToolsResult> {
  if (!isTauri()) {
    return { status: 'unchanged', log: 'Desktop app required for tool installation.' };
  }
  return invoke<InstallToolsResult>('install_huggingface_tools', { request: { installPython, installHf, installLlama } });
}

export async function runHarnessToolAction(presetId: string, action: 'install' | 'check'): Promise<HarnessToolResult> {
  if (!isTauri()) {
    return {
      presetId,
      action,
      status: 'unchanged',
      installCommand: 'Desktop app required for harness setup.',
      checkCommand: 'Desktop app required for harness setup.',
      log: 'Desktop app required for harness setup.',
    };
  }
  return invoke<HarnessToolResult>('run_harness_tool_action', { request: { presetId, action } });
}

export async function searchHuggingFaceModels(query = '', sort = 'trendingScore', limit = 20): Promise<HuggingFaceModel[]> {
  if (!isTauri()) {
    return [
      {
        repoId: 'unsloth/Qwen3.6-35B-A3B-GGUF',
        author: 'unsloth',
        url: 'https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF',
        downloads: 847652,
        likes: 1317,
        trendingScore: 41,
        pipelineTag: 'image-text-to-text',
        libraryName: 'transformers',
        gated: false,
        tags: ['gguf', 'qwen', 'apache-2.0', 'conversational'],
        ggufFiles: ['Qwen3.6-35B-A3B-UD-Q4_K_M.gguf', 'Qwen3.6-35B-A3B-Q8_0.gguf'],
        recommendedFile: 'Qwen3.6-35B-A3B-UD-Q4_K_M.gguf',
      },
    ];
  }
  return invoke<HuggingFaceModel[]>('search_huggingface_models', { request: { query, sort, limit, ggufOnly: true } });
}

export async function inspectHuggingFaceModel(repoId: string, revision?: string): Promise<HuggingFaceModelFiles> {
  if (!isTauri()) {
    const ggufFileDetails = [
      { file: 'Qwen3.6-35B-A3B-UD-Q4_K_M.gguf', sizeBytes: 22 * 1024 ** 3, sha256: null, quantization: 'Q4_K_M' },
      { file: 'Qwen3.6-35B-A3B-Q8_0.gguf', sizeBytes: 37 * 1024 ** 3, sha256: null, quantization: 'Q8_0' },
    ];
    return {
      repoId,
      url: `https://huggingface.co/${repoId}`,
      ggufFiles: ggufFileDetails.map(detail => detail.file),
      ggufFileDetails,
      recommendedFile: ggufFileDetails[0].file,
    };
  }
  return invoke<HuggingFaceModelFiles>('inspect_huggingface_model', { request: { repoId, revision } });
}

export async function planHuggingFaceDownload(repoId: string, filename?: string, revision?: string): Promise<HuggingFaceDownloadPlan> {
  const selectedFile = filename ?? 'model-q4_k_m.gguf';
  if (!isTauri()) {
    return {
      repoId,
      selectedFile,
      revision: revision ?? null,
      localDir: 'browser',
      plannedBytes: 1024,
      existingBytes: null,
      partialBytes: 0,
      alreadyDownloaded: false,
      summary: `Planned download: ${selectedFile} (1.0K)`,
      diskCheck: 'Disk check skipped in browser mode.',
      retryHint: 'Retry: browser mode uses mock download data.',
    };
  }
  return invoke<HuggingFaceDownloadPlan>('plan_huggingface_download', { request: { repoId, filename, revision } });
}

export async function downloadHuggingFaceModel(repoId: string, filename?: string, revision?: string, downloadId?: string): Promise<DownloadedModel> {
  if (!isTauri()) {
    const selectedFile = filename ?? 'model-q4_k_m.gguf';
    return {
      repoId,
      revision: revision ?? null,
      path: 'browser',
      files: [selectedFile],
      ggufFiles: [selectedFile],
      ggufFileDetails: [{ file: selectedFile, sizeBytes: 1024, sha256: 'browser-mock-sha256', quantization: 'Q4_K_M' }],
      sizeBytes: 1024,
      selectedFile,
      downloadLog: 'Browser mock download.',
    };
  }
  return invoke<DownloadedModel>('download_huggingface_model', { request: { repoId, filename, revision, downloadId } });
}

export interface HuggingFaceDownloadJobOptions {
  startAfterDownload?: boolean;
  runConnectivityAfterStart?: boolean;
  autoBenchmarkPackId?: string;
  autoCompareAfterStart?: boolean;
  startPort?: number;
  startContext?: number;
}

export interface HuggingFaceServerJobOptions {
  registerTargetAfterStart?: boolean;
  runConnectivityAfterStart?: boolean;
  autoBenchmarkPackId?: string;
  autoCompareAfterStart?: boolean;
}

export async function startHuggingFaceDownloadJob(repoId: string, filename?: string, revision?: string, options: HuggingFaceDownloadJobOptions = {}): Promise<HuggingFaceDownloadJob> {
  if (!isTauri()) {
    const selectedFile = filename ?? 'model-q4_k_m.gguf';
    return {
      id: crypto.randomUUID(),
      repoId,
      selectedFile,
      status: 'completed',
      message: `Browser mock downloaded ${selectedFile}`,
      startedAt: new Date().toISOString(),
      finishedAt: new Date().toISOString(),
      plannedBytes: 1024,
      transferredBytes: 1024,
      percent: 100,
      localDir: 'browser',
      model: await downloadHuggingFaceModel(repoId, selectedFile, revision),
      startAfterDownload: Boolean(options.startAfterDownload),
      runConnectivityAfterStart: Boolean(options.runConnectivityAfterStart),
      autoBenchmarkPackId: options.autoBenchmarkPackId ?? null,
      autoCompareAfterStart: Boolean(options.autoCompareAfterStart),
      startPort: options.startPort ?? null,
      startContext: options.startContext ?? null,
    };
  }
  return invoke<HuggingFaceDownloadJob>('start_huggingface_download_job', { request: { repoId, filename, revision, ...options } });
}

export async function listHuggingFaceDownloadJobs(): Promise<HuggingFaceDownloadJob[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<HuggingFaceDownloadJob[]>('list_huggingface_download_jobs');
}

export async function getHuggingFaceDownloadJob(id: string): Promise<HuggingFaceDownloadJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceDownloadJob | null>('get_huggingface_download_job', { id });
}

export async function cancelHuggingFaceDownloadJob(id: string): Promise<HuggingFaceDownloadJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceDownloadJob | null>('cancel_huggingface_download_job', { id });
}

export async function retryHuggingFaceDownloadJob(id: string): Promise<HuggingFaceDownloadJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceDownloadJob | null>('retry_huggingface_download_job', { id });
}

export async function clearFinishedHuggingFaceDownloadJobs(): Promise<number> {
  if (!isTauri()) {
    return 0;
  }
  return invoke<number>('clear_finished_huggingface_download_jobs');
}

export async function revealHuggingFaceModel(repoId: string): Promise<void> {
  if (!isTauri()) {
    return;
  }
  return invoke<void>('reveal_huggingface_model', { request: { repoId } });
}

export async function deleteHuggingFaceModel(repoId: string): Promise<HuggingFaceStatus> {
  if (!isTauri()) {
    return huggingFaceStatus();
  }
  return invoke<HuggingFaceStatus>('delete_huggingface_model', { request: { repoId } });
}

export async function preflightHuggingFaceModel(repoId: string, filename: string | undefined, context: number): Promise<ModelPreflight> {
  if (!isTauri()) {
    const modelSizeBytes = 1024 * 1024 * 1024;
    return {
      status: 'ok',
      summary: 'Browser mock preflight passed',
      warnings: [],
      errors: [],
      repoId,
      selectedFile: filename ?? 'model-q4_k_m.gguf',
      modelSizeBytes,
      estimatedMemoryBytes: modelSizeBytes * 2,
      systemMemoryBytes: null,
      context,
    };
  }
  return invoke<ModelPreflight>('preflight_huggingface_model', { request: { repoId, filename, context } });
}

export async function startHuggingFaceModel(repoId: string, filename: string | undefined, port: number, context: number): Promise<HuggingFaceStatus> {
  if (!isTauri()) {
    return huggingFaceStatus();
  }
  return invoke<HuggingFaceStatus>('start_huggingface_model', { request: { repoId, filename, port, context } });
}

export async function startHuggingFaceServerJob(repoId: string, filename: string | undefined, port: number, context: number, options: HuggingFaceServerJobOptions = {}): Promise<HuggingFaceServerJob> {
  if (!isTauri()) {
    const selectedFile = filename ?? 'model-q4_k_m.gguf';
    const serverStatus: HuggingFaceStatus = {
      tokenAvailable: false,
      pythonAvailable: true,
      pythonSupported: true,
      pythonVersion: '3.11.0',
      hfCliAvailable: true,
      llamaServerAvailable: true,
      serverRunning: true,
      serverModelId: selectedFile,
      cacheDir: 'browser/.benchforge/models',
      cacheSizeBytes: 1024,
      cacheFreeBytes: null,
      detail: `Browser mock server ready on 127.0.0.1:${port}`,
      models: [
        {
          repoId,
          revision: null,
          path: 'browser',
          files: [selectedFile],
          ggufFiles: [selectedFile],
          ggufFileDetails: [{ file: selectedFile, sizeBytes: 1024, sha256: null, quantization: null }],
          sizeBytes: 1024,
          selectedFile,
        },
      ],
    };
    return {
      id: crypto.randomUUID(),
      repoId,
      selectedFile,
      port,
      context,
      status: 'completed',
      message: `Browser mock llama-server ready for ${selectedFile}`,
      startedAt: new Date().toISOString(),
      finishedAt: new Date().toISOString(),
      serverStatus,
      registerTargetAfterStart: Boolean(options.registerTargetAfterStart),
      runConnectivityAfterStart: Boolean(options.runConnectivityAfterStart),
      autoBenchmarkPackId: options.autoBenchmarkPackId ?? null,
      autoCompareAfterStart: Boolean(options.autoCompareAfterStart),
    };
  }
  return invoke<HuggingFaceServerJob>('start_huggingface_server_job', { request: { repoId, filename, port, context, ...options } });
}

export async function listHuggingFaceServerJobs(): Promise<HuggingFaceServerJob[]> {
  if (!isTauri()) {
    return [];
  }
  return invoke<HuggingFaceServerJob[]>('list_huggingface_server_jobs');
}

export async function getHuggingFaceServerJob(id: string): Promise<HuggingFaceServerJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceServerJob | null>('get_huggingface_server_job', { id });
}

export async function cancelHuggingFaceServerJob(id: string): Promise<HuggingFaceServerJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceServerJob | null>('cancel_huggingface_server_job', { id });
}

export async function retryHuggingFaceServerJob(id: string): Promise<HuggingFaceServerJob | null> {
  if (!isTauri()) {
    return null;
  }
  return invoke<HuggingFaceServerJob | null>('retry_huggingface_server_job', { id });
}

export async function clearFinishedHuggingFaceServerJobs(): Promise<number> {
  if (!isTauri()) {
    return 0;
  }
  return invoke<number>('clear_finished_huggingface_server_jobs');
}

export async function stopHuggingFaceModel(): Promise<HuggingFaceStatus> {
  if (!isTauri()) {
    return huggingFaceStatus();
  }
  return invoke<HuggingFaceStatus>('stop_huggingface_model');
}

export async function runDoctor(): Promise<DoctorCheck[]> {
  if (!isTauri()) {
    return [
      { id: 'gui-path', label: 'GUI PATH', status: 'ok', detail: 'mock: default app paths configured', category: 'Environment', importance: 'required', remediation: '', command: '' },
      { id: 'git', label: 'Git', status: 'ok', detail: 'mock: installed', category: 'Core', importance: 'recommended', remediation: '', command: 'git --version' },
      { id: 'hf', label: 'Hugging Face CLI', status: 'warn', detail: 'mock: not checked in browser mode', category: 'Local models', importance: 'recommended', remediation: 'Use the desktop app to check and install missing local model tools.', command: 'hf version' },
      { id: 'cloud-key-openai', label: 'OpenAI', status: 'warn', detail: 'mock: not checked in browser mode', category: 'Cloud API keys', importance: 'recommended', remediation: 'Use the desktop app to check Keychain and environment variables.', command: 'OPENAI_API_KEY' },
      { id: 'docker', label: 'Docker/Colima', status: 'warn', detail: 'mock: not checked in browser mode', category: 'Sandbox', importance: 'optional', remediation: 'Use the desktop app to check Docker-backed scoring.', command: 'docker --version' },
      { id: 'codex', label: 'Codex CLI', status: 'warn', detail: 'mock: not checked in browser mode', category: 'Agent CLIs', importance: 'optional', remediation: 'Install Codex CLI before running Codex adapter benchmarks.', command: 'codex --version' },
    ];
  }
  return invoke<DoctorCheck[]>('run_doctor');
}

function sampleBrowserResults(): RunResult[] {
  const now = new Date().toISOString();
  const rows = [
    ['browser-run-1', 'browser-group-a', 'qwen-ollama', 'llm-core', 'llm-core-classification-001', 'passed', 1, 420, 152, 52, 123.8, 200, 0],
    ['browser-run-2', 'browser-group-a', 'qwen-ollama', 'llm-core', 'llm-core-extraction-001', 'passed', 1, 510, 188, 61, 119.6, 200, 0],
    ['browser-run-3', 'browser-group-a', 'qwen-ollama', 'llm-core', 'llm-core-tool-call-001', 'failed', 0, 650, 210, 70, 107.7, 422, 0],
    ['browser-run-4', 'browser-group-a', 'mock-agent', 'llm-core', 'llm-core-classification-001', 'passed', 1, 80, 120, 30, 375.0, null, 0],
    ['browser-run-5', 'browser-group-a', 'mock-agent', 'llm-core', 'llm-core-extraction-001', 'passed', 1, 92, 122, 32, 347.8, null, 0],
    ['browser-run-6', 'browser-group-a', 'mock-agent', 'llm-core', 'llm-core-tool-call-001', 'passed', 1, 88, 130, 28, 318.2, null, 0],
    ['browser-run-7', 'browser-group-b', 'cloud-gpt', 'llm-basics', 'llm-json-validity-001', 'passed', 1, 900, 240, 80, 88.9, 200, 0.0003],
    ['browser-run-8', 'browser-group-b', 'cloud-gpt', 'llm-basics', 'llm-summarization-001', 'passed', 0.9, 980, 260, 86, 87.8, 200, 0.00032],
  ] as const;
  return rows.map(([id, groupId, targetId, benchmarkPackId, taskId, status, score, wallTimeMs, promptTokens, completionTokens, outputTokensPerSecond, httpStatus, costUsd]) => ({
    id,
    run_group_id: groupId,
    targetId,
    benchmarkPackId,
    taskId,
    status,
    score,
    wallTimeMs,
    prompt_tokens: promptTokens,
    completion_tokens: completionTokens,
    total_tokens: promptTokens + completionTokens,
    provider_attempts: status === 'failed' ? 2 : 1,
    provider_retry_after_ms: status === 'failed' ? 2000 : null,
    http_status: httpStatus,
    output_tokens_per_second: outputTokensPerSecond,
    cost_usd: costUsd,
    started_at: now,
    finished_at: now,
    error_code: status === 'failed' ? 'malformed_response' : null,
    error_message: status === 'failed' ? 'browser mock failure' : null,
    reproducibility: { browserMock: true },
  }));
}
