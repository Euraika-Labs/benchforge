export type TargetKind = 'direct_model' | 'harnessed_model' | 'cli_agent' | 'benchmark_harness' | 'mock';

export interface Target {
  id: string;
  name: string;
  kind: TargetKind;
  adapterId: string;
  model?: string | null;
  endpoint?: string | null;
  command?: string | null;
  status: 'unknown' | 'valid' | 'invalid';
  enabled: boolean;
  isLocalModel?: boolean;
  isCloudModel?: boolean;
  validationStatus?: string | null;
  validationDetail?: string | null;
  validationCheckedAt?: string | null;
  inputPriceUsdPerMillionTokens?: number | null;
  outputPriceUsdPerMillionTokens?: number | null;
  cacheReadPriceUsdPerMillionTokens?: number | null;
  cacheWritePriceUsdPerMillionTokens?: number | null;
}

export interface RedactedTargetExport {
  id: string;
  name: string;
  kind: TargetKind | string;
  adapter_id: string;
  config: Record<string, unknown>;
}

export interface TargetValidation {
  targetId: string;
  status: 'ok' | 'warn' | 'error' | string;
  detail: string;
  checkedAt: string;
}

export interface CreateTargetBenchmarkHandoffResult {
  target: Target;
  validation?: TargetValidation | null;
  runJob?: RunJob | null;
  benchmarkError?: string | null;
}

export interface LocalRuntime {
  id: string;
  name: string;
  adapterId: string;
  baseUrl: string;
  status: 'ok' | 'warn' | 'error' | string;
  detail: string;
  probeUrl?: string | null;
  modelSource?: string | null;
  detectedAt: string;
  models: string[];
  recommendedModel?: string | null;
  installCommand?: string;
  startCommand?: string;
  modelHint?: string;
  setupHint?: string;
}

export interface LocalRuntimeToolResult {
  runtimeId: string;
  action: 'install' | 'check' | 'pull' | string;
  status: 'ready' | 'partial' | 'missing' | 'unchanged' | string;
  installCommand?: string | null;
  checkCommand: string;
  log: string;
}

export interface CloudModel {
  model: string;
  name: string;
  provider: string;
  inputPriceUsdPerMillionTokens?: number | null;
  outputPriceUsdPerMillionTokens?: number | null;
  cacheReadPriceUsdPerMillionTokens?: number | null;
  cacheWritePriceUsdPerMillionTokens?: number | null;
  contextLength?: number | null;
  source: string;
  sourceUrl?: string | null;
  detail?: string | null;
}

export interface Adapter {
  id: string;
  name: string;
  kind: string;
  adapterVersion: number;
  schemaVersion: number;
  defaultBaseUrl?: string;
  command?: string;
  path: string;
  validationStatus: 'ok' | 'warn' | 'error';
  validationDetail: string;
  capabilities: Record<string, boolean>;
  metadata: Record<string, unknown>;
}

export interface BenchmarkPack {
  id: string;
  name: string;
  version: string;
  description?: string | null;
  tags: string[];
  estimatedRuntime?: string | null;
  requiresSandbox: boolean;
  tasks: number;
  promptTasks: number;
  totalTaskWeight: number;
  heavy: boolean;
  taskTypes: string[];
  languages: string[];
  requiredTools: string[];
  scoringMethods: string[];
  supportedTargetKinds: string[];
  targetFit: string;
  evidenceProfile: string;
  evidenceWarnings: string[];
  calibrationStatus: string;
  calibrationSampleSize?: number | null;
  calibrationBaselineModels: string[];
  calibrationLastReviewed?: string | null;
  calibrationReviewScope?: string | null;
  calibrationQualityGates?: string[];
  calibrationNotes?: string | null;
  source: string;
  sourcePath: string;
}

export interface BenchmarkPackDiagnostic {
  id?: string | null;
  source: string;
  sourcePath: string;
  status: 'ok' | 'warn' | 'error' | string;
  detail: string;
}

export interface BenchmarkPackTask {
  id: string;
  name: string;
  taskType: string;
  language?: string | null;
  fixture?: string | null;
  prompt: string;
  timeoutSeconds: number;
  maxTurns?: number | null;
  weight: number;
  scoringMethods: string[];
  scoring: Record<string, unknown>;
  sourcePath: string;
}

export interface CreatedBenchmarkPackTemplate {
  pack: BenchmarkPack;
  sourcePath: string;
  taskPath: string;
}

export interface AddedBenchmarkPackPromptTask {
  pack: BenchmarkPack;
  sourcePath: string;
  taskPath: string;
  taskId: string;
}

export interface UpdatedBenchmarkPackPromptTask {
  pack: BenchmarkPack;
  sourcePath: string;
  taskPath: string;
  taskId: string;
}

export interface UpdatedBenchmarkPackCalibration {
  pack: BenchmarkPack;
  sourcePath: string;
}

export interface BenchmarkPackCalibrationSuggestion {
  packId: string;
  status: string;
  sampleSize: number;
  baselineModels: string[];
  lastReviewed?: string | null;
  notes: string;
  targetCount: number;
  taskCount: number;
  runGroupCount: number;
  warnings: string[];
}

export interface ScorePromptTaskPreview {
  status: string;
  score: number;
  tests: Record<string, unknown>;
  errorMessage?: string | null;
  scoringMethods: string[];
}

export interface DeletedBenchmarkPackTask {
  pack: BenchmarkPack;
  sourcePath: string;
  deletedTaskId: string;
  deletedTaskPath: string;
}

export interface ExportedBenchmarkPack {
  pack: BenchmarkPack;
  sourcePath: string;
  exportPath: string;
  format: 'folder' | 'zip' | string;
  filesCopied: number;
}

export interface ImportedBenchmarkPack {
  pack: BenchmarkPack;
  sourcePath: string;
  importPath: string;
  filesCopied: number;
}

export interface DoctorCheck {
  id: string;
  label: string;
  status: 'ok' | 'warn' | 'error';
  detail: string;
  category: string;
  importance: 'required' | 'recommended' | 'optional';
  remediation: string;
  command: string;
}

export interface DiagnosticRecord {
  id: string;
  kind: string;
  level: 'debug' | 'info' | 'warn' | 'error' | string;
  message: string;
  detail?: string | null;
  createdAt: string;
  logPath: string;
}

export interface DiagnosticEventRequest {
  kind: string;
  level?: 'debug' | 'info' | 'warn' | 'error' | string;
  message: string;
  detail?: string | null;
}

export interface HarnessToolResult {
  presetId: string;
  action: string;
  status: 'ready' | 'partial' | 'missing' | 'unchanged' | string;
  installCommand: string;
  checkCommand: string;
  log: string;
}

export interface RunResult {
  id: string;
  run_group_id?: string | null;
  targetId: string;
  target_id?: string;
  benchmarkPackId: string;
  benchmark_pack_id?: string;
  taskId: string;
  task_id?: string;
  status: string;
  pass_fail?: boolean | null;
  score?: number | null;
  score_numeric?: number | null;
  wallTimeMs?: number | null;
  wall_time_ms?: number | null;
  setup_time_ms?: number | null;
  target_time_ms?: number | null;
  evaluation_time_ms?: number | null;
  model_call_wall_time_ms?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  reasoning_tokens?: number | null;
  cached_tokens?: number | null;
  cache_read_tokens?: number | null;
  cache_write_tokens?: number | null;
  total_tokens?: number | null;
  estimated_cost_usd?: number | null;
  provider_attempts?: number | null;
  provider_retry_after_ms?: number | null;
  provider_retry_delay_ms?: number | null;
  http_status?: number | null;
  provider_time_to_first_byte_ms?: number | null;
  ttft_ms?: number | null;
  provider_time_to_first_token_ms?: number | null;
  provider_request_total_ms?: number | null;
  decode_tokens_per_sec?: number | null;
  output_tokens_per_second?: number | null;
  peak_rss_mb?: number | null;
  exit_code?: number | null;
  harness_exit_code?: number | null;
  stdout_bytes?: number | null;
  stderr_bytes?: number | null;
  files_changed?: number | null;
  lines_added?: number | null;
  lines_deleted?: number | null;
  commands_observed_count?: number | null;
  dangerous_command_hits?: number | null;
  security_finding_count?: number | null;
  security_files_scanned?: number | null;
  import_file_count?: number | null;
  import_total_file_count?: number | null;
  import_omitted_file_count?: number | null;
  import_truncated?: number | null;
  import_truncated_bytes?: number | null;
  provider_model?: string | null;
  provider_model_source?: string | null;
  finish_reason?: string | null;
  pricing_assumption?: string | null;
  import_format?: string | null;
  import_source?: string | null;
  import_path?: string | null;
  summary_source?: string | null;
  cost_usd?: number | null;
  started_at?: string | null;
  finished_at?: string | null;
  error_code?: string | null;
  error_message?: string | null;
  reproducibility?: Record<string, unknown>;
  artifacts?: string[];
  warnings?: string[];
  error?: string | null;
}

export interface RunJob {
  id: string;
  runGroupId: string;
  benchmarkPackId: string;
  status: 'queued' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed' | string;
  message: string;
  startedAt: string;
  finishedAt?: string | null;
  total: number;
  completed: number;
  results: RunResult[];
  error?: string | null;
  settings?: RunJobSettings;
}

export interface RunJobSettings {
  targetCount: number;
  taskCount: number;
  repetitions: number;
  warmupRuns: number;
  concurrency: number;
  docker: boolean;
  maxCostUsd?: number | null;
  replay?: RunJobReplay | null;
}

export interface RunJobReplay {
  mode: string;
  sourceJobId: string;
  sourceRunGroupId: string;
  sourceTargetCount: number;
  sourceTaskCount: number;
  sourceRepetitions: number;
  targetCount: number;
  taskCount: number;
  repetitions: number;
  scoped: boolean;
}

export interface RunEstimate {
  targetCount: number;
  taskCount: number;
  repetitions: number;
  warmupRuns: number;
  concurrency: number;
  measuredRuns: number;
  warmupCalls: number;
  totalModelCalls: number;
  estimatedPromptTokens: number;
  estimatedMaxCompletionTokens: number;
  estimatedMaxCostUsd?: number | null;
  estimatedMeasuredTimeoutSeconds: number;
  estimatedWarmupTimeoutSeconds: number;
  estimatedWallClockTimeoutSeconds: number;
  pricedTargets: number;
  unpricedTargets: string[];
  heavy: boolean;
  notes: string[];
}

export interface Artifact {
  id: string;
  run_id: string;
  kind: string;
  path: string;
  mime_type?: string | null;
  size_bytes?: number | null;
  sha256?: string | null;
  metadata: Record<string, unknown>;
}

export interface DownloadedModel {
  repoId: string;
  revision?: string | null;
  path: string;
  files: string[];
  ggufFiles: string[];
  ggufFileDetails: GgufFileDetail[];
  sizeBytes: number;
  selectedFile?: string | null;
  downloadLog?: string | null;
}

export interface HuggingFaceDownloadPlan {
  repoId: string;
  selectedFile: string;
  revision?: string | null;
  localDir: string;
  plannedBytes?: number | null;
  existingBytes?: number | null;
  partialBytes: number;
  alreadyDownloaded: boolean;
  summary: string;
  diskCheck: string;
  retryHint: string;
}

export interface HuggingFaceDownloadProgress {
  downloadId?: string | null;
  repoId: string;
  selectedFile: string;
  status: 'planned' | 'running' | 'completed' | 'error' | 'cancelled';
  message: string;
  localDir: string;
  transferredBytes: number;
  plannedBytes?: number | null;
  percent?: number | null;
}

export interface HuggingFaceDownloadJob {
  id: string;
  repoId: string;
  selectedFile?: string | null;
  status: 'queued' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed' | string;
  message: string;
  startedAt: string;
  finishedAt?: string | null;
  plannedBytes?: number | null;
  transferredBytes: number;
  percent?: number | null;
  localDir?: string | null;
  error?: string | null;
  model?: DownloadedModel | null;
  startAfterDownload?: boolean;
  runConnectivityAfterStart?: boolean;
  autoBenchmarkPackId?: string | null;
  autoCompareAfterStart?: boolean;
  autoBenchmarkTargetIds?: string[];
  startPort?: number | null;
  startContext?: number | null;
}

export interface HuggingFaceServerJob {
  id: string;
  repoId: string;
  selectedFile?: string | null;
  port: number;
  context: number;
  status: 'queued' | 'running' | 'cancelling' | 'cancelled' | 'completed' | 'failed' | string;
  message: string;
  startedAt: string;
  finishedAt?: string | null;
  error?: string | null;
  serverStatus?: HuggingFaceStatus | null;
  registerTargetAfterStart?: boolean;
  runConnectivityAfterStart?: boolean;
  autoBenchmarkPackId?: string | null;
  autoCompareAfterStart?: boolean;
  autoBenchmarkTargetIds?: string[];
}

export interface GgufFileDetail {
  file: string;
  sizeBytes: number;
  sha256?: string | null;
  quantization?: string | null;
}

export interface HuggingFaceStatus {
  tokenAvailable: boolean;
  pythonAvailable: boolean;
  pythonSupported: boolean;
  pythonVersion?: string | null;
  hfCliAvailable: boolean;
  llamaServerAvailable: boolean;
  serverRunning: boolean;
  serverModelId?: string | null;
  cacheDir: string;
  cacheSizeBytes: number;
  cacheFreeBytes?: number | null;
  detail: string;
  models: DownloadedModel[];
}

export interface ModelPreflight {
  status: 'ok' | 'warn' | 'error' | string;
  summary: string;
  warnings: string[];
  errors: string[];
  repoId: string;
  selectedFile: string;
  modelSizeBytes: number;
  estimatedMemoryBytes: number;
  systemMemoryBytes?: number | null;
  context: number;
}

export interface HuggingFaceModel {
  repoId: string;
  author?: string | null;
  url: string;
  downloads: number;
  likes: number;
  trendingScore?: number | null;
  pipelineTag?: string | null;
  libraryName?: string | null;
  gated: boolean;
  lastModified?: string | null;
  tags: string[];
  ggufFiles: string[];
  recommendedFile?: string | null;
}

export interface HuggingFaceModelFiles {
  repoId: string;
  url: string;
  ggufFiles: string[];
  ggufFileDetails: GgufFileDetail[];
  recommendedFile?: string | null;
}

export interface InstallToolsResult {
  status: 'ready' | 'partial' | 'unchanged' | string;
  log: string;
}
