import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  Activity,
  Boxes,
  ClipboardCheck,
  Copy,
  Database,
  Download,
  ExternalLink,
  FlaskConical,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Square,
  TerminalSquare,
  Trash2,
  Upload,
  Wrench,
} from 'lucide-react';
import {
  addBenchmarkPackPromptTask,
  cancelHuggingFaceServerJob,
  cancelHuggingFaceDownloadJob,
  cancelRunJob,
  clearFinishedHuggingFaceServerJobs,
  clearFinishedHuggingFaceDownloadJobs,
  clearFinishedRunJobs,
  createBenchmarkPackTemplate,
  createTarget,
  createTargetWithBenchmarkHandoff,
  deleteTarget,
  deleteBenchmarkPackTask,
  deleteHuggingFaceModel,
  detectLocalRuntimes,
  duplicateTarget,
  estimateRunPlan,
  exportBenchmarkPack,
  listDiagnostics,
  duplicateRunJob,
  exportTargetRedacted,
  exportReportFolder,
  exportResults,
  getRunJob,
  getHuggingFaceServerJob,
  getHuggingFaceDownloadJob,
  huggingFaceStatus,
  importBenchmarkPack,
  installHuggingFaceTools,
  inspectHuggingFaceModel,
  isDesktopRuntime,
  listAdapters,
  listArtifacts,
  listBenchmarkPackDiagnostics,
  listBenchmarkPacks,
  listBenchmarkPackTasks,
  listHuggingFaceServerJobs,
  listHuggingFaceDownloadJobs,
  listResults,
  listRunJobs,
  listTargets,
  planHuggingFaceDownload,
  preflightHuggingFaceModel,
  providerApiKeyStatus,
  readArtifact,
  recordDiagnosticEvent,
  revealHuggingFaceModel,
  retryHuggingFaceDownloadJob,
  retryHuggingFaceServerJob,
  retryRunJob,
  runHarnessToolAction,
  runLocalRuntimeToolAction,
  runDoctor,
  runWorkerMock,
  saveHuggingFaceToken,
  saveProviderApiKey,
  scorePromptTaskPreview,
  searchCloudModels,
  searchHuggingFaceModels,
  setTargetEnabled,
  suggestBenchmarkPackCalibration,
  startHuggingFaceServerJob,
  startHuggingFaceDownloadJob,
  startRunJob,
  stopHuggingFaceModel,
  updateBenchmarkPackCalibration,
  updateBenchmarkPackPromptTask,
  validateTarget,
} from './api';
import type { Adapter, Artifact, BenchmarkPack, BenchmarkPackDiagnostic, BenchmarkPackTask, CloudModel, DiagnosticRecord, DoctorCheck, DownloadedModel, GgufFileDetail, HarnessToolResult, HuggingFaceDownloadJob, HuggingFaceDownloadPlan, HuggingFaceDownloadProgress, HuggingFaceModel, HuggingFaceServerJob, HuggingFaceStatus, LocalRuntime, LocalRuntimeToolResult, ModelPreflight, RunEstimate, RunJob, RunResult, ScorePromptTaskPreview, Target, TargetValidation } from './types';

type Page = 'dashboard' | 'targets' | 'benchmarks' | 'runs' | 'results' | 'doctor' | 'settings';
type DateWindow = 'all' | 'today' | '7d' | '30d';

interface ModelPreset {
  id: string;
  label: string;
  model: string;
  inputPrice?: number;
  outputPrice?: number;
  cacheReadPrice?: number;
  cacheWritePrice?: number;
  contextLength?: number;
  source?: string;
  note?: string;
}

interface HarnessPreset {
  id: string;
  label: string;
  targetId: string;
  targetName: string;
  command: string;
  timeoutSeconds: string;
  benchmarkPackId: string;
  installCommand: string;
  checkCommand: string;
  setupHint: string;
  outputHint: string;
  modelPlaceholder?: string;
  baseUrlPlaceholder?: string;
  defaultModel?: string;
  envPassthrough?: string[];
  tags: string[];
}

interface RunBuilderIntent {
  targetIds: string[];
  benchmarkPackId?: string;
  taskIds?: string[];
  repetitions?: number;
  warmupRuns?: number;
  concurrency?: number;
  maxCostUsd?: number;
}

interface TargetRepairIntent {
  targetIds: string[];
  code: string;
  nonce: number;
}

interface TargetSetupIntent {
  adapterId?: string;
  benchmarkPackId?: string;
  targetIds?: string[];
  code: string;
  nonce: number;
}

interface HuggingFaceLocalSetupIntent {
  benchmarkPackId?: string;
  targetIds?: string[];
  nonce: number;
}

interface ResultsScopeIntent {
  groupId: string;
  runId?: string;
  nonce: number;
}

interface LocalCloudRunReadiness {
  tone: 'ok' | 'warn';
  headline: string;
  facts: string[];
  notes: string[];
  recommendedCostCapUsd?: number;
  pricingRepairTargetIds: string[];
}

interface BenchmarkPackOption {
  id: string;
  label: string;
}

interface HfAutomaticHandoffStep {
  label: string;
  detail: string;
  tone: 'ok' | 'warn' | 'unknown';
  optional?: boolean;
}

const dateWindowOptions: { id: DateWindow; label: string }[] = [
  { id: 'all', label: 'All time' },
  { id: 'today', label: 'Today' },
  { id: '7d', label: 'Last 7 days' },
  { id: '30d', label: 'Last 30 days' },
];

const missingProviderModelKey = '__missing_provider_model__';
const missingProviderKey = '__missing_provider__';
const firstRunSeenKey = 'benchforge-first-run-seen';
const emptyWorkspaceDoctorOpenedKey = 'benchforge-empty-workspace-doctor-opened';
const automaticConnectivityMaxCostUsd = 0.05;
const defaultComparisonMaxCostUsd = 1.0;
const connectivityBenchmarkPackId = 'llm-connectivity';
const defaultModelComparisonPackId = 'llm-basics';
const dashboardPrimaryComparisonTargetLimit = 4;
const preferredCloudSetupAdapterIds = ['openai', 'anthropic', 'mistral', 'gemini', 'openrouter', 'azure-openai', 'openai-compatible'];
const emptyDoctorChecks: DoctorCheck[] = [];
const fallbackModelBenchmarkPacks = [
  { id: 'llm-basics', label: 'LLM Basics' },
  { id: 'llm-core', label: 'LLM Core' },
  { id: 'llm-structured-output', label: 'LLM Structured Output' },
  { id: 'llm-grounded-context', label: 'LLM Grounded Context' },
  { id: 'llm-practical', label: 'LLM Practical Selection' },
  { id: 'llm-decision-suite', label: 'LLM Decision Suite' },
  { id: 'llm-reliability', label: 'LLM Reliability' },
  { id: 'llm-connectivity', label: 'LLM Connectivity' },
];

const modelBenchmarkPackOrder = [
  'llm-basics',
  'llm-core',
  'llm-structured-output',
  'llm-grounded-context',
  'llm-practical',
  'llm-decision-suite',
  'llm-reliability',
  'llm-connectivity',
];

const modelSelectionPackPreference = [
  'llm-basics',
  'llm-core',
  'llm-structured-output',
  'llm-grounded-context',
  'llm-practical',
  'llm-decision-suite',
  'llm-reliability',
];

const harnessPresets: HarnessPreset[] = [
  {
    id: 'evalplus',
    label: 'EvalPlus',
    targetId: 'evalplus-local',
    targetName: 'EvalPlus local harness',
    command: 'python3 -m evalplus.evaluate --dataset {dataset} --samples {workspace}/samples.jsonl',
    timeoutSeconds: '7200',
    benchmarkPackId: 'evalplus',
    installCommand: 'python3 -m pip install evalplus',
    checkCommand: 'python3 -m evalplus.evaluate --help',
    setupHint: 'Generate EvalPlus-compatible samples into {workspace}/samples.jsonl before running, or replace the command with your own generator/evaluator wrapper.',
    outputHint: 'Emit JSON with total/passed/failed/score, or text containing pass@1/accuracy and passed counts.',
    tags: ['HumanEval+', 'MBPP+', 'code'],
  },
  {
    id: 'aider-polyglot',
    label: 'Aider Polyglot',
    targetId: 'aider-polyglot-local',
    targetName: 'Aider Polyglot local harness',
    command: 'python3 -m aider.benchmark --model {model} --subset {subset}',
    timeoutSeconds: '7200',
    benchmarkPackId: 'aider-polyglot-subset',
    installCommand: 'python3 -m pip install aider-chat',
    checkCommand: 'python3 -m aider --version',
    setupHint: 'Set model to the provider/model name used by your Aider setup and list any required provider key names in Env passthrough.',
    outputHint: 'Emit JSON score fields, or text with score/pass@1/accuracy and passed counts.',
    modelPlaceholder: 'openai/gpt-4.1-mini or local provider alias',
    defaultModel: 'openai/gpt-4.1-mini',
    envPassthrough: ['OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'MISTRAL_API_KEY', 'OPENROUTER_API_KEY', 'AZURE_OPENAI_API_KEY'],
    tags: ['repo edits', 'polyglot', 'agent'],
  },
  {
    id: 'terminal-bench',
    label: 'Terminal-Bench',
    targetId: 'terminal-bench-local',
    targetName: 'Terminal-Bench local harness',
    command: 'tb run --model {model} --subset {subset}',
    timeoutSeconds: '10800',
    benchmarkPackId: 'terminal-bench-subset',
    installCommand: 'python3 -m pip install terminal-bench',
    checkCommand: 'tb --help',
    setupHint: 'Set model to the agent/model identifier accepted by your Terminal-Bench installation and list any required provider key names in Env passthrough.',
    outputHint: 'Emit JSON score fields, or text with score/pass@1/accuracy and passed counts.',
    modelPlaceholder: 'provider/model or local agent name',
    envPassthrough: ['OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'MISTRAL_API_KEY', 'OPENROUTER_API_KEY', 'AZURE_OPENAI_API_KEY'],
    tags: ['terminal', 'agent', 'tasks'],
  },
  {
    id: 'swebench',
    label: 'SWE-bench Lite',
    targetId: 'swebench-lite-local',
    targetName: 'SWE-bench Lite local harness',
    command: 'python3 -m swebench.harness.run_evaluation --model_name_or_path {model} --subset {subset}',
    timeoutSeconds: '14400',
    benchmarkPackId: 'swebench-lite-subset',
    installCommand: 'python3 -m pip install swebench',
    checkCommand: 'python3 -m swebench.harness.run_evaluation --help',
    setupHint: 'Use a model name or local path accepted by your SWE-bench harness. Keep dataset/cache directories outside the app bundle and list any required provider key names in Env passthrough.',
    outputHint: 'Emit JSON with resolved/unresolved, total/passed/failed, or score fields.',
    modelPlaceholder: 'model name or local model path',
    envPassthrough: ['OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'MISTRAL_API_KEY', 'OPENROUTER_API_KEY', 'AZURE_OPENAI_API_KEY', 'HF_TOKEN'],
    tags: ['repo repair', 'lite', 'patches'],
  },
];

const nav = [
  { id: 'dashboard' as Page, label: 'Dashboard', icon: Activity },
  { id: 'targets' as Page, label: 'Targets', icon: Boxes },
  { id: 'benchmarks' as Page, label: 'Benchmarks', icon: FlaskConical },
  { id: 'runs' as Page, label: 'Runs', icon: TerminalSquare },
  { id: 'results' as Page, label: 'Results', icon: ClipboardCheck },
  { id: 'doctor' as Page, label: 'Doctor', icon: ShieldCheck },
  { id: 'settings' as Page, label: 'Settings', icon: Settings },
];

export default function App() {
  const desktopRuntime = isDesktopRuntime();
  const [page, setPage] = useState<Page>(() => (localStorage.getItem(firstRunSeenKey) ? 'dashboard' : 'doctor'));
  const [targets, setTargets] = useState<Target[]>([]);
  const [adapters, setAdapters] = useState<Adapter[]>([]);
  const [packs, setPacks] = useState<BenchmarkPack[]>([]);
  const [packDiagnostics, setPackDiagnostics] = useState<BenchmarkPackDiagnostic[]>([]);
  const [checks, setChecks] = useState<DoctorCheck[]>([]);
  const [diagnostics, setDiagnostics] = useState<DiagnosticRecord[]>([]);
  const [results, setResults] = useState<RunResult[]>([]);
  const [runJobs, setRunJobs] = useState<RunJob[]>([]);
  const [downloadJobs, setDownloadJobs] = useState<HuggingFaceDownloadJob[]>([]);
  const [serverJobs, setServerJobs] = useState<HuggingFaceServerJob[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string>('');
  const [artifactText, setArtifactText] = useState('');
  const [runBuilderIntent, setRunBuilderIntent] = useState<RunBuilderIntent | null>(null);
  const [targetRepairIntent, setTargetRepairIntent] = useState<TargetRepairIntent | null>(null);
  const [targetSetupIntent, setTargetSetupIntent] = useState<TargetSetupIntent | null>(null);
  const [hfLocalSetupIntent, setHfLocalSetupIntent] = useState<HuggingFaceLocalSetupIntent | null>(null);
  const [resultsScopeIntent, setResultsScopeIntent] = useState<ResultsScopeIntent | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');

  async function refresh() {
    const [nextTargets, nextAdapters, nextPacks, nextPackDiagnostics, nextChecks, nextResults, nextDiagnostics, nextRunJobs, nextDownloadJobs, nextServerJobs] = await Promise.all([
      listTargets(),
      listAdapters(),
      listBenchmarkPacks(),
      listBenchmarkPackDiagnostics(),
      runDoctor(),
      listResults(),
      listDiagnostics().catch(() => []),
      listRunJobs(),
      listHuggingFaceDownloadJobs(),
      listHuggingFaceServerJobs(),
    ]);
    setTargets(nextTargets);
    setAdapters(nextAdapters);
    setPacks(nextPacks);
    setPackDiagnostics(nextPackDiagnostics);
    setChecks(nextChecks);
    setResults(nextResults);
    setDiagnostics(nextDiagnostics);
    setRunJobs(nextRunJobs);
    setDownloadJobs(nextDownloadJobs);
    setServerJobs(nextServerJobs);
    if (
      benchmarkWorkspaceNeedsSetup(nextTargets, nextResults, nextRunJobs, nextDownloadJobs, nextServerJobs)
      && !localStorage.getItem(emptyWorkspaceDoctorOpenedKey)
    ) {
      const setupIntent = firstUsefulTargetSetupIntent(nextAdapters, nextChecks, nextPacks);
      if (setupIntent) {
        setTargetRepairIntent(null);
        setHfLocalSetupIntent(null);
        setTargetSetupIntent({ ...setupIntent, nonce: Date.now() });
        setPage('targets');
      } else {
        setPage('doctor');
      }
      localStorage.setItem(emptyWorkspaceDoctorOpenedKey, '1');
    }
    localStorage.setItem(firstRunSeenKey, '1');
  }

  useEffect(() => {
    refresh().catch(error => {
      setMessage(String(error));
      recordAppDiagnostic('frontend_refresh', error);
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let sawActiveWork = false;

    async function pollWorkQueues() {
      try {
        const [nextRunJobs, nextDownloadJobs, nextServerJobs] = await Promise.all([
          listRunJobs(),
          listHuggingFaceDownloadJobs(),
          listHuggingFaceServerJobs(),
        ]);
        if (cancelled) {
          return;
        }
        const hasActiveWork = nextRunJobs.some(isJobActive)
          || nextDownloadJobs.some(isDownloadJobActive)
          || nextServerJobs.some(isServerJobActive);
        setRunJobs(nextRunJobs);
        setDownloadJobs(nextDownloadJobs);
        setServerJobs(nextServerJobs);
        if (hasActiveWork || sawActiveWork) {
          listResults()
            .then(nextResults => {
              if (!cancelled) {
                setResults(nextResults);
              }
            })
            .catch(error => {
              if (!cancelled) {
                recordAppDiagnostic('work_results_poll', error);
              }
            });
        }
        sawActiveWork = hasActiveWork;
        timer = setTimeout(pollWorkQueues, hasActiveWork ? 1000 : 5000);
      } catch (error) {
        if (!cancelled) {
          setMessage(String(error));
          recordAppDiagnostic('work_queue_poll', error);
          timer = setTimeout(pollWorkQueues, 5000);
        }
      }
    }

    timer = setTimeout(pollWorkQueues, 5000);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    function onWindowError(event: ErrorEvent) {
      recordDiagnosticEvent({
        kind: 'frontend_error',
        level: 'error',
        message: event.message || 'Unhandled frontend error',
        detail: event.error?.stack || `${event.filename}:${event.lineno}:${event.colno}`,
      }).then(record => setDiagnostics(current => [record, ...current].slice(0, 25))).catch(() => undefined);
    }
    function onUnhandledRejection(event: PromiseRejectionEvent) {
      recordDiagnosticEvent({
        kind: 'frontend_rejection',
        level: 'error',
        message: diagnosticMessage(event.reason),
        detail: diagnosticDetail(event.reason),
      }).then(record => setDiagnostics(current => [record, ...current].slice(0, 25))).catch(() => undefined);
    }
    window.addEventListener('error', onWindowError);
    window.addEventListener('unhandledrejection', onUnhandledRejection);
    return () => {
      window.removeEventListener('error', onWindowError);
      window.removeEventListener('unhandledrejection', onUnhandledRejection);
    };
  }, []);

  useEffect(() => {
    const runId = selectedRunId || results[0]?.id || '';
    setSelectedRunId(runId);
    if (!runId) {
      setArtifacts([]);
      return;
    }
    listArtifacts(runId).then(setArtifacts).catch(error => {
      setMessage(String(error));
      recordAppDiagnostic('artifact_load', error);
    });
  }, [selectedRunId, results]);

  const selectedResult = results.find(result => result.id === selectedRunId);

  function openRunBuilder(intent: RunBuilderIntent) {
    setRunBuilderIntent(intent);
    setPage('runs');
  }

  function openTargetRepair(intent: Omit<TargetRepairIntent, 'nonce'>) {
    setTargetRepairIntent({ ...intent, nonce: Date.now() });
    setTargetSetupIntent(null);
    setPage('targets');
  }

  function openTargetSetup(intent: Omit<TargetSetupIntent, 'nonce'>) {
    setTargetSetupIntent({ ...intent, nonce: Date.now() });
    setTargetRepairIntent(null);
    setPage('targets');
  }

  function openHuggingFaceLocalSetup(intent: Omit<HuggingFaceLocalSetupIntent, 'nonce'> = {}) {
    setHfLocalSetupIntent({ ...intent, nonce: Date.now() });
    setPage('settings');
  }

  function openResultsForGroup(groupId: string, runId?: string) {
    setResultsScopeIntent({ groupId, runId, nonce: Date.now() });
    setPage('results');
  }

  const content = useMemo(() => {
    switch (page) {
      case 'targets':
        return <Targets targets={targets} adapters={adapters} packs={packs} checks={checks} onRefresh={refresh} setMessage={setMessage} openRunBuilder={openRunBuilder} openResultsForGroup={openResultsForGroup} repairIntent={targetRepairIntent} onRepairIntentConsumed={() => setTargetRepairIntent(null)} setupIntent={targetSetupIntent} onSetupIntentConsumed={() => setTargetSetupIntent(null)} />;
      case 'benchmarks':
        return <Benchmarks packs={packs} diagnostics={packDiagnostics} onRefresh={refresh} setMessage={setMessage} />;
      case 'runs':
        return <Runs targets={targets} adapters={adapters} packs={packs} busy={busy} setBusy={setBusy} setMessage={setMessage} refresh={refresh} setPage={setPage} openResultsForGroup={openResultsForGroup} openTargetRepair={openTargetRepair} openTargetSetup={openTargetSetup} openHuggingFaceLocalSetup={openHuggingFaceLocalSetup} runBuilderIntent={runBuilderIntent} onRunBuilderIntentConsumed={() => setRunBuilderIntent(null)} />;
      case 'results':
        return (
          <Results
            results={results}
            targets={targets}
            adapters={adapters}
            packs={packs}
            artifacts={artifacts}
            selectedRunId={selectedRunId}
            setSelectedRunId={setSelectedRunId}
            selectedResult={selectedResult}
            artifactText={artifactText}
            setArtifactText={setArtifactText}
            setMessage={setMessage}
            scopeIntent={resultsScopeIntent}
            openRunBuilder={openRunBuilder}
            openTargetRepair={openTargetRepair}
          />
        );
      case 'doctor':
        return <Doctor checks={checks} diagnostics={diagnostics} targets={targets} adapters={adapters} packs={packs} onRefresh={refresh} setBusy={setBusy} setMessage={setMessage} setPage={setPage} openRunBuilder={openRunBuilder} openTargetRepair={openTargetRepair} openTargetSetup={openTargetSetup} openHuggingFaceLocalSetup={openHuggingFaceLocalSetup} />;
      case 'settings':
        return <SettingsPage busy={busy} targets={targets} adapters={adapters} packs={packs} setBusy={setBusy} setMessage={setMessage} refresh={refresh} openRunBuilder={openRunBuilder} openResultsForGroup={openResultsForGroup} openTargetRepair={openTargetRepair} openTargetSetup={openTargetSetup} setupIntent={hfLocalSetupIntent} onSetupIntentConsumed={() => setHfLocalSetupIntent(null)} />;
      default:
        return <Dashboard targets={targets} adapters={adapters} packs={packs} checks={checks} results={results} runJobs={runJobs} downloadJobs={downloadJobs} serverJobs={serverJobs} busy={busy} setBusy={setBusy} refresh={refresh} setPage={setPage} setMessage={setMessage} openRunBuilder={openRunBuilder} openTargetRepair={openTargetRepair} openTargetSetup={openTargetSetup} openHuggingFaceLocalSetup={openHuggingFaceLocalSetup} />;
    }
  }, [page, targets, adapters, packs, packDiagnostics, checks, diagnostics, results, runJobs, downloadJobs, serverJobs, artifacts, selectedRunId, selectedResult, artifactText, runBuilderIntent, targetRepairIntent, targetSetupIntent, hfLocalSetupIntent, resultsScopeIntent, busy]);

  return (
    <div className="shell">
      <aside>
        <div className="brand"><Database size={22} /> BenchForge</div>
        {nav.map(item => {
          const Icon = item.icon;
          return <button key={item.id} className={page === item.id ? 'active' : ''} onClick={() => setPage(item.id)}><Icon size={18}/>{item.label}</button>;
        })}
      </aside>
      <main>
        <div className="topbar">
          <button className="icon-button" title="Refresh" onClick={() => refresh().catch(error => {
            setMessage(String(error));
            recordAppDiagnostic('manual_refresh', error);
          })}><RefreshCw size={17} /></button>
          {message && <span className="status-text">{message}</span>}
        </div>
        {!desktopRuntime ? <div className="runtime-banner">
          <strong>Browser preview</strong>
          <span>Desktop backend is not connected. Local downloads, Keychain storage, provider calls, and benchmark runs use sample data here.</span>
        </div> : null}
        {content}
      </main>
    </div>
  );
}

function Dashboard({ targets, adapters, packs, checks, results, runJobs, downloadJobs, serverJobs, busy, setBusy, refresh, setPage, setMessage, openRunBuilder, openTargetRepair, openTargetSetup, openHuggingFaceLocalSetup }: { targets: Target[]; adapters: Adapter[]; packs: BenchmarkPack[]; checks: DoctorCheck[]; results: RunResult[]; runJobs: RunJob[]; downloadJobs: HuggingFaceDownloadJob[]; serverJobs: HuggingFaceServerJob[]; busy: boolean; setBusy: (busy: boolean) => void; refresh: () => Promise<void>; setPage: (page: Page) => void; setMessage: (message: string) => void; openRunBuilder: (intent: RunBuilderIntent) => void; openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void; openTargetSetup: (intent: Omit<TargetSetupIntent, 'nonce'>) => void; openHuggingFaceLocalSetup: (intent?: Omit<HuggingFaceLocalSetupIntent, 'nonce'>) => void }) {
  const errors = checks.filter(c => c.status === 'error').length;
  const warnings = checks.filter(c => c.status === 'warn').length;
  const passed = results.filter(result => result.status === 'passed').length;
  const adapterById = useMemo(() => new Map(adapters.map(adapter => [adapter.id, adapter])), [adapters]);
  const targetById = useMemo(() => new Map(targets.map(target => [target.id, target])), [targets]);
  const modelSelectionResults = useMemo(
    () => results.filter(result => dashboardResultIsModelSelectionResult(result, targetById)),
    [results, targetById],
  );
  const comparisonRows = useMemo(() => buildComparisonRows(modelSelectionResults, adapterById), [modelSelectionResults, adapterById]);
  const taskRows = useMemo(() => buildTaskComparisonRows(modelSelectionResults), [modelSelectionResults]);
  const targetRankings = useMemo(() => buildTargetRankingRows(modelSelectionResults, adapterById), [modelSelectionResults, adapterById]);
  const packEvidenceIssues = useMemo(() => buildPackEvidenceIssues(modelSelectionResults, packs), [modelSelectionResults, packs]);
  const packCalibrationIssues = useMemo(() => buildPackCalibrationIssues(modelSelectionResults), [modelSelectionResults]);
  const bestTarget = targetRankings[0];
  const dashboardEvidence = useMemo(
    () => targetRankings.length ? comparisonEvidenceAssessment(comparisonRows, taskRows, targetRankings, packEvidenceIssues, packCalibrationIssues) : null,
    [comparisonRows, taskRows, targetRankings, packEvidenceIssues, packCalibrationIssues],
  );
  const weeklyCost = dashboardRecentCost(modelSelectionResults, 7);
  const localCheck = dashboardCheck(checks, 'benchmark-target-local', 'Local model target', 'warn', 'Add a local model target');
  const cloudCheck = dashboardCheck(checks, 'benchmark-target-cloud', 'Cloud model target', 'warn', 'Add a cloud model target');
  const compareCheck = dashboardCheck(checks, 'benchmark-local-cloud-compare', 'Local + cloud comparison', 'warn', 'Add one local and one cloud target');
  const comparisonResultCheck = dashboardCheck(checks, 'benchmark-local-cloud-results', 'Local + cloud result', 'warn', compareCheck.status === 'ok' ? 'Run a local + cloud comparison' : 'Add one local and one cloud target');
  const comparisonEvidenceCheck = dashboardCheck(checks, 'benchmark-local-cloud-evidence', 'Evidence quality', 'warn', comparisonResultCheck.status === 'ok' ? 'Run 3 repetitions per task/target' : 'Run a local + cloud comparison');
  const nextBenchmarkStep = dashboardCheck(checks, 'benchmark-next-step', 'Next benchmark step', 'warn', 'Add one local model target and one cloud model target');
  const packCheck = dashboardCheck(checks, 'benchmark-packs', 'Benchmark packs', packs.length ? 'ok' : 'error', packs.length ? `${packs.length} packs available` : 'No packs found');
  const liveCloudCheck = dashboardCheck(checks, 'product-live-cloud', 'Live cloud validation', 'warn', 'Validate a real cloud target');
  const distributionCheck = dashboardCheck(checks, 'product-distribution', 'Public distribution', 'warn', 'Run signed/notarized release validation');
  const liveCloudTargetIds = useMemo(
    () => targets.filter(target => targetIsSelectableModel(target) && isCloudModelTarget(target)).map(target => target.id),
    [targets],
  );
  const localRuntimeCheck = dashboardLocalRuntimeCheck(checks);
  const sandboxCheck = dashboardSandboxCheck(checks);
  const cloudSetupAdapterId = usePreferredCloudSetupAdapterId(adapters, checks);
  const recommendedTargets = dashboardLocalCloudComparisonTargets(targets);
  const recommendedTargetIds = recommendedTargets.runTargetIds;
  const allComparableTargetIds = recommendedTargets.allRunTargetIds;
  const primaryComparisonTargetIds = dashboardPrimaryComparisonTargetIds(recommendedTargetIds, allComparableTargetIds);
  const primaryComparisonIncludesAllComparable = primaryComparisonTargetIds.length >= 2
    && primaryComparisonTargetIds.length === allComparableTargetIds.length;
  const setupLocalTargetIds = recommendedTargets.setupLocalTargetIds;
  const setupCloudTargetIds = recommendedTargets.setupCloudTargetIds;
  const pricingRepairTargetIds = recommendedTargets.pricingRepairTargetIds;
  const skippedUnpricedCloudTargetIds = recommendedTargets.skippedUnpricedCloudTargetIds;
  const comparisonReady = compareCheck.status === 'ok';
  const comparisonNeedsPricing = comparisonReady && Boolean(pricingRepairTargetIds.length) && recommendedTargetIds.length < 2;
  const recommendedComparisonPack = useMemo(() => recommendedComparisonPackId(packs), [packs]);
  const hasReliabilityPack = packs.some(pack => pack.id === 'llm-reliability');
  const activeRunInProgress = runJobs.some(isJobActive);
  const dashboardEvidencePricingRepairTargetIds = useMemo(
    () => dashboardEvidence ? evidencePricingRepairTargetIds(dashboardEvidence, targetRankings, targetById) : [],
    [dashboardEvidence, targetRankings, targetById],
  );
  const dashboardEvidenceNextRunIntent = useMemo(
    () => dashboardEvidence && !dashboardEvidencePricingRepairTargetIds.length
      ? evidenceNextRunIntent(dashboardEvidence, targetRankings, targets, packs)
      : null,
    [dashboardEvidence, dashboardEvidencePricingRepairTargetIds, targetRankings, targets, packs],
  );
  const primaryBenchmarkActionLabel = comparisonNeedsPricing ? 'Add cloud pricing' : comparisonReady ? dashboardPrimaryComparisonActionLabel(primaryComparisonTargetIds, allComparableTargetIds) : dashboardBenchmarkStepLabel(nextBenchmarkStep);
  const primaryBenchmarkActionDisabled = busy || (comparisonReady && !comparisonNeedsPricing && activeRunInProgress);
  const primaryBenchmarkActionTitle = activeRunInProgress && comparisonReady
    ? 'A benchmark job is already running'
    : comparisonReady && !comparisonNeedsPricing && primaryComparisonIncludesAllComparable
      ? `Runs ${primaryComparisonTargetIds.length === 2 ? 'both' : `all ${primaryComparisonTargetIds.length}`} comparable local/priced cloud targets with the same pack and capped cost.${skippedUnpricedCloudTargetIds.length ? ` Skips unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}.` : ''}`
      : comparisonReady && !comparisonNeedsPricing && allComparableTargetIds.length > primaryComparisonTargetIds.length
      ? `Runs the recommended pair: ${previewList(recommendedTargetIds)}. Use Compare all for all ${allComparableTargetIds.length} comparable priced targets.${skippedUnpricedCloudTargetIds.length ? ` Skips unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}.` : ''}`
      : undefined;
  const compareAllAvailable = comparisonReady && !comparisonNeedsPricing && allComparableTargetIds.length > primaryComparisonTargetIds.length;
  const compareAllDisabled = busy || activeRunInProgress || !compareAllAvailable;
  const compareAllTitle = activeRunInProgress
    ? 'A benchmark job is already running'
    : compareAllAvailable
      ? `Validate and run all ${allComparableTargetIds.length} comparable local/priced cloud targets${skippedUnpricedCloudTargetIds.length ? `; skips unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}` : ''}`
      : comparisonNeedsPricing && pricingRepairTargetIds.length
        ? `Add input/output pricing before comparing every local/cloud target: ${previewList(pricingRepairTargetIds)}`
      : comparisonReady && !comparisonNeedsPricing && primaryComparisonIncludesAllComparable
        ? `${primaryBenchmarkActionLabel} already includes ${primaryComparisonTargetIds.length === 2 ? 'both comparable targets' : `all ${primaryComparisonTargetIds.length} comparable targets`}`
      : 'Add more comparable local or priced cloud targets before comparing all models';
  const reliabilityComparisonDisabled = busy || activeRunInProgress || !comparisonReady || comparisonNeedsPricing || !hasReliabilityPack;
  const reliabilityComparisonTitle = activeRunInProgress
    ? 'A benchmark job is already running'
    : busy
      ? 'BenchForge is busy'
      : !hasReliabilityPack
        ? 'LLM Reliability pack is not available'
        : comparisonNeedsPricing
          ? `Add input/output pricing before running a capped reliability comparison: ${previewList(pricingRepairTargetIds)}`
          : !comparisonReady
            ? 'Add one enabled local model target and one enabled priced cloud model target'
            : `Run ${benchmarkPackLabel('llm-reliability')} with ${primaryComparisonTargetIds.length} comparable local/cloud target(s), 3 repetitions, 1 warmup, and ${formatCost(defaultComparisonMaxCostUsd)} cap`;
  const localSetupPrefersRuntimeDetection = localRuntimeCheck.check.status === 'ok';
  const localSetupLabel = localSetupPrefersRuntimeDetection ? 'Detect runtime' : 'Local setup';
  const localSetupTitle = localSetupPrefersRuntimeDetection
    ? 'Doctor found a reachable local runtime endpoint; detect its models and add it to the next benchmark'
    : 'Open the managed Hugging Face GGUF workflow to search, download, start, and benchmark a local model';
  function openLocalRuntimeDetection() {
    openTargetSetup({ code: 'local_runtime_detect', benchmarkPackId: recommendedComparisonPack, targetIds: setupCloudTargetIds });
    const comparisonNote = setupCloudTargetIds.length ? ` to compare with ${previewList(setupCloudTargetIds)}` : '';
    setMessage(`Detecting local runtimes from Dashboard${comparisonNote}`);
  }
  function openSmartLocalSetup() {
    if (localSetupPrefersRuntimeDetection) {
      openLocalRuntimeDetection();
      return;
    }
    openHuggingFaceLocalModelSetup();
  }
  function openCloudTargetSetup() {
    openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId: recommendedComparisonPack, targetIds: setupLocalTargetIds });
  }
  async function validateDashboardCloudTargets() {
    if (!liveCloudTargetIds.length) {
      openCloudTargetSetup();
      setMessage('Add a cloud target before running live cloud validation');
      return;
    }
    setBusy(true);
    try {
      setMessage(`Validating ${liveCloudTargetIds.length} cloud target(s) with live provider probes`);
      const validationResults = await Promise.all(liveCloudTargetIds.map(id => validateTarget(id)));
      await refresh();
      const blockers = validationResults.filter(result => result.status === 'error');
      if (blockers.length) {
        openTargetRepair({ targetIds: blockers.map(blocker => blocker.targetId), code: validationRepairCode(blockers[0]) });
        setMessage(`Cloud validation found ${formatValidationCodeCounts(blockers)}. Fix the affected target before running live comparisons.`);
        return;
      }
      const warnings = validationResults.filter(result => result.status !== 'ok');
      if (warnings.length) {
        setMessage(`Cloud validation finished with warnings: ${formatValidationCodeCounts(warnings)}`);
        return;
      }
      setMessage(`Validated ${validationResults.length} cloud target(s); product readiness can count this as live cloud evidence.`);
    } catch (error) {
      setMessage(`Cloud validation failed: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  }
  function openHuggingFaceLocalModelSetup() {
    openHuggingFaceLocalSetup({ benchmarkPackId: recommendedComparisonPack, targetIds: setupCloudTargetIds });
    setMessage(huggingFaceLocalModelSetupMessage(targets, setupCloudTargetIds, recommendedComparisonPack));
  }
  async function runDashboardIntent(intent: RunBuilderIntent, scopeLabelOverride = 'local/cloud comparison') {
    const benchmarkPackId = intent.benchmarkPackId ?? recommendedComparisonPack;
    const selectedTaskIds = intent.taskIds?.filter(id => id.trim()) ?? [];
    setBusy(true);
    try {
      const selectedTaskNote = selectedTaskIds.length ? ` with ${selectedTaskIds.length} selected task(s)` : '';
      setMessage(`Validating ${intent.targetIds.length} target(s) before starting ${benchmarkPackLabel(benchmarkPackId)}${selectedTaskNote}`);
      const validationResults = await Promise.all(intent.targetIds.map(id => validateTarget(id)));
      await refresh();
      const blockers = validationResults.filter(result => result.status === 'error');
      if (blockers.length) {
        openTargetRepair({ targetIds: blockers.map(blocker => blocker.targetId), code: validationRepairCode(blockers[0]) });
        setMessage(`Run blocked: ${formatValidationCodeCounts(blockers)}. Fix validation errors before starting the comparison.`);
        return;
      }
      const warningNote = validationResults.some(result => result.status !== 'ok') ? `; warnings: ${formatValidationCodeCounts(validationResults.filter(result => result.status !== 'ok'))}` : '';
      const scopeNote = scopeLabelOverride;
      const skippedPricingNote = skippedUnpricedCloudTargetIds.length ? `. Skipped unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}` : '';
      setMessage(`Starting ${benchmarkPackLabel(benchmarkPackId)} ${scopeNote} with ${intent.targetIds.length} target(s), ${intent.repetitions} repetitions, ${intent.warmupRuns} warmup${selectedTaskNote}, ${formatCost(intent.maxCostUsd ?? defaultComparisonMaxCostUsd)} cap${warningNote}${skippedPricingNote}`);
      const job = await startRunJob(
        intent.targetIds,
        false,
        benchmarkPackId,
        intent.repetitions ?? 3,
        intent.warmupRuns ?? 1,
        intent.concurrency ?? 1,
        intent.maxCostUsd ?? defaultComparisonMaxCostUsd,
        selectedTaskIds,
      );
      await refresh();
      if (isJobActive(job)) {
        setPage('runs');
        setMessage(`Started ${scopeNote} job ${job.id.slice(0, 8)} with ${job.total} planned run(s)`);
        return;
      }
      if (job.results.length || job.status === 'completed') {
        setPage('results');
        setMessage(`${job.completed}/${job.total} ${scopeNote} benchmark task runs completed`);
        return;
      }
      setPage('runs');
      setMessage(job.message || `${scopeNote} job ${job.id.slice(0, 8)} finished without stored results`);
    } catch (error) {
      setMessage(benchmarkRunFailureMessage(error));
      openRunBuilder(intent);
    } finally {
      setBusy(false);
    }
  }
  async function runDashboardComparison(
    benchmarkPackId: string,
    targetIds = primaryComparisonTargetIds,
    scopeLabelOverride = '',
  ) {
    const intent = localCloudRunBuilderIntent(targetIds, benchmarkPackId);
    const scopeNote = scopeLabelOverride || (allComparableTargetIds.length > intent.targetIds.length ? 'recommended pair' : 'local/cloud comparison');
    await runDashboardIntent(intent, scopeNote);
  }
  function runDashboardEvidenceFollowUp(intent: RunBuilderIntent) {
    void runDashboardIntent(intent, 'evidence follow-up');
  }
  function openDashboardEvidencePricingRepair(targetIds: string[]) {
    openTargetRepair({ targetIds, code: 'pricing_assumption' });
    setMessage(`Add pricing before running the recommended evidence follow-up: ${previewList(targetIds)}`);
  }
  async function retryActiveWork(row: ActiveWorkRow) {
    setBusy(true);
    try {
      setMessage(`Retrying ${row.kind.toLowerCase()} ${row.shortId}`);
      switch (row.workType) {
        case 'run':
          await retryRunJob(row.id);
          break;
        case 'download':
          await retryHuggingFaceDownloadJob(row.id);
          break;
        case 'server':
          await retryHuggingFaceServerJob(row.id);
          break;
      }
      await refresh();
      setPage(row.page);
      setMessage(`Retried ${row.kind.toLowerCase()} ${row.shortId}; opened ${row.page === 'runs' ? 'Runs' : 'local jobs'} to track progress`);
    } catch (error) {
      setMessage(row.workType === 'run' ? benchmarkRunFailureMessage(error) : String(error));
    } finally {
      setBusy(false);
    }
  }
  async function cancelActiveWork(row: ActiveWorkRow) {
    setBusy(true);
    try {
      setMessage(`Stopping ${row.kind.toLowerCase()} ${row.shortId}`);
      switch (row.workType) {
        case 'run':
          await cancelRunJob(row.id);
          break;
        case 'download':
          await cancelHuggingFaceDownloadJob(row.id);
          break;
        case 'server':
          await cancelHuggingFaceServerJob(row.id);
          break;
      }
      await refresh();
      setMessage(`Stop requested for ${row.kind.toLowerCase()} ${row.shortId}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  function openComparisonRun(benchmarkPackId = recommendedComparisonPack) {
    if ((comparisonReady || nextBenchmarkStep.command.startsWith('Runs > Local + cloud')) && primaryComparisonTargetIds.length >= 2) {
      void runDashboardComparison(benchmarkPackId);
      return;
    }
    if ((comparisonReady || nextBenchmarkStep.command.startsWith('Runs > Local + cloud')) && pricingRepairTargetIds.length) {
      openTargetRepair({ targetIds: pricingRepairTargetIds, code: 'pricing_assumption' });
      setMessage(`Add input/output pricing before running a capped local/cloud comparison: ${previewList(pricingRepairTargetIds)}`);
      return;
    }
    const repairSide = readinessRepairSide(nextBenchmarkStep);
    if (repairSide) {
      const targetIds = failedReadinessRepairTargetIds(targets, repairSide);
      if (targetIds.length) {
        openTargetRepair({ targetIds, code: readinessRepairCode(targets, repairSide) });
        setMessage(`Repairing failed ${repairSide} target: ${previewList(targetIds)}`);
        return;
      }
    }
    if (nextBenchmarkStep.command.startsWith('Settings') && localRuntimeCheck.check.status === 'ok') {
      openLocalRuntimeDetection();
      return;
    }
    if (nextBenchmarkStep.command.startsWith('Targets')) {
      openCloudTargetSetup();
      return;
    }
    setPage(nextBenchmarkStep.command ? nextBenchmarkStepPage(nextBenchmarkStep) : 'doctor');
  }
  function openAllComparisonRun() {
    if (!compareAllAvailable) {
      if (pricingRepairTargetIds.length) {
        openTargetRepair({ targetIds: pricingRepairTargetIds, code: 'pricing_assumption' });
        setMessage(`Add input/output pricing before comparing every local/cloud target: ${previewList(pricingRepairTargetIds)}`);
        return;
      }
      setMessage('Add at least one local model and one priced cloud model before comparing every target');
      return;
    }
    const skippedPricingNote = skippedUnpricedCloudTargetIds.length ? ` Skipped unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}.` : '';
    setMessage(`Starting all comparable local/cloud target(s) with ${benchmarkPackLabel(recommendedComparisonPack)}, 3 repetitions, 1 warmup, and ${formatCost(defaultComparisonMaxCostUsd)} cap.${skippedPricingNote}`);
    void runDashboardComparison(recommendedComparisonPack, allComparableTargetIds, 'all comparable local/cloud comparison');
  }
  function openSkippedCloudPricing() {
    openTargetRepair({ targetIds: skippedUnpricedCloudTargetIds, code: 'pricing_assumption' });
    setMessage(`Add input/output pricing to include skipped cloud target(s) in capped local/cloud comparisons: ${previewList(skippedUnpricedCloudTargetIds)}`);
  }
  return <section><h1>Dashboard</h1><div className="grid">
    <Card title="Targets" value={targets.length} note="configured" />
    <Card title="Benchmark packs" value={packs.length} note="available" />
    <Card title="Doctor" value={errors ? `${errors}` : 'Ready'} note={errors ? `${warnings} warnings` : `${warnings} warnings`} />
    <Card title="Runs" value={`${passed}/${results.length}`} note="passed" />
    <Card title={dashboardEvidence?.grade === 'comparison_ready' ? 'Selected target' : 'Leading target'} value={bestTarget ? compactDashboardValue(bestTarget.targetId) : '-'} note={bestTarget ? dashboardLeaderNote(bestTarget, dashboardEvidence) : 'no results yet'} />
    <Card title="LLM cost 7d" value={formatCost(weeklyCost.costUsd)} note={weeklyCost.pricedRows ? `${weeklyCost.pricedRows}/${weeklyCost.rows} priced rows` : `${weeklyCost.rows} recent LLM rows`} />
    <Card title="Local runtimes" value={localRuntimeCheck.value} note={localRuntimeCheck.note} />
    <Card title="Sandbox" value={sandboxCheck.value} note={sandboxCheck.note} />
  </div>
    <BenchmarkNextStepPanel
      checks={checks}
      openBenchmarkStep={() => openComparisonRun()}
      primaryDisabled={primaryBenchmarkActionDisabled}
      primaryTitle={primaryBenchmarkActionTitle}
      primaryLabel={primaryBenchmarkActionLabel}
      primaryIcon={comparisonNeedsPricing ? <Pencil size={14} /> : comparisonReady ? <Play size={14} /> : dashboardBenchmarkStepIcon(nextBenchmarkStep)}
    />
    <DashboardModelSelectionPanel
      rankings={targetRankings}
      evidence={dashboardEvidence}
      recentCost={weeklyCost}
      nextRunIntent={dashboardEvidenceNextRunIntent}
      pricingRepairTargetIds={dashboardEvidencePricingRepairTargetIds}
      busy={busy || activeRunInProgress}
      setPage={setPage}
      onRunNextEvidence={runDashboardEvidenceFollowUp}
      onRepairPricing={openDashboardEvidencePricingRepair}
      emptyActionLabel={primaryBenchmarkActionLabel}
      emptyActionTitle={primaryBenchmarkActionTitle}
      emptyActionDisabled={primaryBenchmarkActionDisabled}
      onEmptyAction={() => openComparisonRun()}
    />
    <ActiveWorkPanel
      runJobs={runJobs}
      downloadJobs={downloadJobs}
      serverJobs={serverJobs}
      busy={busy}
      setPage={setPage}
      onRetry={retryActiveWork}
      onCancel={cancelActiveWork}
    />
    <div className="panel readiness-panel">
      <div className="panel-head"><h2>Benchmark Readiness</h2><button onClick={() => setPage('doctor')}><ShieldCheck size={14} />Doctor</button></div>
      <div className="readiness-steps">
        {[localRuntimeCheck.check, sandboxCheck.check, localCheck, cloudCheck, compareCheck, comparisonResultCheck, comparisonEvidenceCheck, packCheck, liveCloudCheck, distributionCheck].map(check => <div key={check.id} className="readiness-step">
          <span className={`pill ${check.status}`}>{check.status}</span>
          <strong>{check.label}</strong>
          <span>{check.detail}</span>
        </div>)}
      </div>
      {comparisonReady && skippedUnpricedCloudTargetIds.length ? <div className="preflight-box warn">
        <div className="panel-head"><h2>Cloud Pricing</h2><button onClick={openSkippedCloudPricing}><Pencil size={14} />Add pricing</button></div>
        <p>Unpriced cloud target(s) are skipped by capped comparison shortcuts: {previewList(skippedUnpricedCloudTargetIds)}.</p>
      </div> : null}
      <div className="actions">
        <button title={localSetupTitle} onClick={openSmartLocalSetup}>{localSetupPrefersRuntimeDetection ? <Search size={14} /> : <Wrench size={14} />}{localSetupLabel}</button>
        <button onClick={openCloudTargetSetup}><Boxes size={14} />Cloud target</button>
        <button disabled={busy || activeRunInProgress} title={liveCloudTargetIds.length ? `Validate ${liveCloudTargetIds.length} configured cloud target(s) with real provider probes` : 'Add a cloud target before validating live provider access'} onClick={() => validateDashboardCloudTargets().catch(error => setMessage(String(error)))}><ShieldCheck size={14} />{liveCloudTargetIds.length ? 'Validate cloud' : 'Add cloud target'}</button>
        <button disabled={primaryBenchmarkActionDisabled} title={primaryBenchmarkActionTitle} onClick={() => openComparisonRun()}>{comparisonNeedsPricing ? <Pencil size={14} /> : comparisonReady ? <Play size={14} /> : dashboardBenchmarkStepIcon(nextBenchmarkStep)}{busy && comparisonReady ? 'Starting' : primaryBenchmarkActionLabel}</button>
        <button disabled={compareAllDisabled} title={compareAllTitle} onClick={openAllComparisonRun}><ClipboardCheck size={14} />Compare all</button>
        <button disabled={reliabilityComparisonDisabled} title={reliabilityComparisonTitle} onClick={() => openComparisonRun('llm-reliability')}><FlaskConical size={14} />Reliability comparison</button>
        <button disabled={comparisonResultCheck.status !== 'ok'} onClick={() => setPage('results')}><ClipboardCheck size={14} />Results</button>
      </div>
    </div>
  </section>;
}

interface DashboardRecentCost {
  rows: number;
  pricedRows: number;
  costUsd: number | null;
}

function DashboardModelSelectionPanel({
  rankings,
  evidence,
  recentCost,
  nextRunIntent,
  pricingRepairTargetIds,
  busy,
  setPage,
  onRunNextEvidence,
  onRepairPricing,
  emptyActionLabel,
  emptyActionTitle,
  emptyActionDisabled,
  onEmptyAction,
}: {
  rankings: TargetRankingRow[];
  evidence: ComparisonEvidenceAssessment | null;
  recentCost: DashboardRecentCost;
  nextRunIntent: RunBuilderIntent | null;
  pricingRepairTargetIds: string[];
  busy: boolean;
  setPage: (page: Page) => void;
  onRunNextEvidence: (intent: RunBuilderIntent) => void;
  onRepairPricing: (targetIds: string[]) => void;
  emptyActionLabel: string;
  emptyActionTitle?: string;
  emptyActionDisabled: boolean;
  onEmptyAction: () => void;
}) {
  const leaders = rankings.slice(0, 3);
  const canImproveEvidence = Boolean(evidence && evidence.grade !== 'comparison_ready');
  const nextRunTitle = nextRunIntent
    ? `Validate and run ${nextRunIntent.targetIds.length} target(s)${nextRunIntent.taskIds?.length ? ` with ${nextRunIntent.taskIds.length} selected task(s)` : ''}`
    : pricingRepairTargetIds.length
      ? `Add pricing for ${previewList(pricingRepairTargetIds)} before running the evidence follow-up`
      : canImproveEvidence
        ? 'Open Results for detailed evidence follow-up options'
        : 'Evidence is already comparison-ready';
  return <div className="panel dashboard-ranking-panel">
    <div className="panel-head"><h2>Model Selection</h2><div className="actions">
      {!leaders.length ? <button disabled={emptyActionDisabled} title={emptyActionTitle} onClick={onEmptyAction}><ClipboardCheck size={14} />{emptyActionLabel}</button> : null}
      {canImproveEvidence && nextRunIntent ? <button disabled={busy} title={busy ? 'A benchmark job is already running' : nextRunTitle} onClick={() => onRunNextEvidence(nextRunIntent)}><Play size={14} />Improve evidence</button> : null}
      {canImproveEvidence && !nextRunIntent && pricingRepairTargetIds.length ? <button title={nextRunTitle} onClick={() => onRepairPricing(pricingRepairTargetIds)}><Wrench size={14} />Repair pricing</button> : null}
      {canImproveEvidence && !nextRunIntent && !pricingRepairTargetIds.length ? <button title={nextRunTitle} onClick={() => setPage('results')}><ClipboardCheck size={14} />Evidence details</button> : null}
      <button onClick={() => setPage('results')}><ClipboardCheck size={14} />Results</button>
    </div></div>
    {leaders.length ? <>
      <p className="muted">Uses the same weighted target ranking and evidence grading as Results across stored benchmark history.</p>
      {evidence ? <DashboardEvidenceSummary evidence={evidence} /> : null}
      <table className="compact-table dashboard-ranking-table"><thead><tr><th>Rank</th><th>Target</th><th>Evidence</th><th>Score</th><th>Latency</th><th>Cost</th><th>Coverage</th></tr></thead><tbody>
        {leaders.map((row, index) => <tr key={row.targetId}><td>{index + 1}</td><td><strong>{row.targetId}</strong><div className="muted">{row.providers}</div></td><td>{formatPercent(row.weightedPassRate ?? row.passRate)} weighted pass<div className="muted">{formatPercent(row.passRate)} pass, CI {formatPercentRange(row.passRateCiLow, row.passRateCiHigh)}</div></td><td>{formatNumberWithSpread(row.weightedAvgScore ?? row.avgScore, row.scoreStdDev)}</td><td>{formatMs(row.p95TimeMs)} p95</td><td>{formatCost(row.avgCostUsd)}</td><td>{row.runs} run(s), {row.packs} pack(s), {row.tasks} task(s)</td></tr>)}
      </tbody></table>
      <p className="muted">Recent spend: {formatCost(recentCost.costUsd)} across {recentCost.pricedRows}/{recentCost.rows} priced result row(s) from the last 7 days.</p>
    </> : <p className="muted">No stored local/cloud LLM benchmark results yet. Run the same LLM pack against one local and one cloud target to start building comparison evidence.</p>}
  </div>;
}

function DashboardEvidenceSummary({ evidence }: { evidence: ComparisonEvidenceAssessment }) {
  const visibleRisks = evidence.risks.slice(0, 6);
  const hiddenRiskCount = Math.max(0, evidence.risks.length - visibleRisks.length);
  return <div className="evidence-summary">
    <div className="evidence-summary-main">
      <span className={`pill ${evidence.tone}`}>{evidence.label}</span>
      <span>{dashboardEvidenceNote(evidence)}</span>
    </div>
    {visibleRisks.length ? <div className="evidence-risk-row">
      {visibleRisks.map(risk => <span key={risk} className="mini-tag warn" title={dashboardEvidenceRiskHint(risk)}>{dashboardEvidenceRiskLabel(risk)}</span>)}
      {hiddenRiskCount ? <span className="mini-tag" title={evidence.risks.slice(visibleRisks.length).map(dashboardEvidenceRiskLabel).join(', ')}>+{hiddenRiskCount} more</span> : null}
    </div> : null}
    <p className="muted"><strong>Next run:</strong> {evidence.minimumNextRun}</p>
  </div>;
}

function dashboardLeaderNote(row: TargetRankingRow, evidence: ComparisonEvidenceAssessment | null) {
  const score = `${formatPercent(row.weightedPassRate ?? row.passRate)} weighted pass, ${row.runs} runs`;
  if (!evidence) {
    return score;
  }
  if (evidence.grade === 'comparison_ready') {
    return `${evidence.label}: ${score}`;
  }
  return `${evidence.label}: provisional, ${row.runs} runs`;
}

function dashboardEvidenceNote(evidence: ComparisonEvidenceAssessment) {
  if (evidence.grade === 'comparison_ready') {
    return 'The leading target is selected only after matching coverage, run groups, cost coverage, repetition depth, and interval separation.';
  }
  return 'Ranking is provisional until the blockers below are resolved.';
}

function dashboardEvidenceRiskLabel(risk: string) {
  switch (risk) {
    case 'no_comparison_results':
      return 'No comparison results';
    case 'single_target':
      return 'Single target';
    case 'connectivity_pack_only':
      return 'Connectivity only';
    case 'pack_evidence_profile':
      return 'Weak pack evidence';
    case 'low_repetitions':
      return 'Low repetitions';
    case 'coverage_gap':
      return 'Coverage gap';
    case 'separate_run_groups':
      return 'Separate run groups';
    case 'pass_rate_ci_overlap':
      return 'Close confidence interval';
    case 'cost_coverage_gap':
      return 'Missing cost';
    case 'pricing_assumption':
      return 'Pricing assumption';
    case 'provider_model_missing':
      return 'Missing served model';
    case 'provider_model_inconsistent':
      return 'Mixed served model';
    case 'provider_model_configured_fallback':
      return 'Fallback model id';
    case 'generation_settings_mixed':
      return 'Mixed generation settings';
    case 'pack_calibration':
      return 'Pack calibration';
    default:
      return risk.replace(/_/g, ' ');
  }
}

function dashboardEvidenceRiskHint(risk: string) {
  switch (risk) {
    case 'no_comparison_results':
      return 'Run at least one local and one cloud target on the same LLM pack.';
    case 'single_target':
      return 'One target can validate setup, but it cannot compare local versus cloud models.';
    case 'connectivity_pack_only':
      return 'Connectivity packs prove setup, not model quality.';
    case 'pack_evidence_profile':
      return 'The visible pack is not strong enough for model selection.';
    case 'low_repetitions':
      return `Run at least ${recommendedTaskRepetitions} measured repetitions per task and target.`;
    case 'coverage_gap':
      return 'Compared targets need the same pack/task slots.';
    case 'separate_run_groups':
      return 'Run the compared targets together so they share one run-group context.';
    case 'pass_rate_ci_overlap':
      return 'The leader is too close to another target to call a decisive winner.';
    case 'cost_coverage_gap':
      return 'Add pricing or token usage so cost can be compared.';
    case 'pricing_assumption':
      return 'Cache or token pricing used a fallback assumption.';
    case 'provider_model_missing':
      return 'Provider or runtime did not confirm the served model id.';
    case 'provider_model_inconsistent':
      return 'One target appears to have served more than one model id.';
    case 'provider_model_configured_fallback':
      return 'BenchForge used the configured model id because the provider did not report one.';
    case 'generation_settings_mixed':
      return 'Targets were compared with different temperature, top-p, or seed policy.';
    case 'pack_calibration':
      return 'The pack needs documented calibration before it can select a winner.';
    default:
      return 'Evidence blocker';
  }
}

interface ActiveWorkRow {
  key: string;
  id: string;
  shortId: string;
  workType: 'run' | 'download' | 'server';
  kind: string;
  label: string;
  status: string;
  active: boolean;
  retryable: boolean;
  cancellable: boolean;
  progress: string;
  percent: number | null;
  message: string;
  startedAt: string;
  page: Page;
}

function ActiveWorkPanel({
  runJobs,
  downloadJobs,
  serverJobs,
  busy,
  setPage,
  onRetry,
  onCancel,
}: {
  runJobs: RunJob[];
  downloadJobs: HuggingFaceDownloadJob[];
  serverJobs: HuggingFaceServerJob[];
  busy: boolean;
  setPage: (page: Page) => void;
  onRetry: (row: ActiveWorkRow) => void;
  onCancel: (row: ActiveWorkRow) => void;
}) {
  const rows = buildActiveWorkRows(runJobs, downloadJobs, serverJobs);
  const activeCount = rows.filter(row => row.active).length;
  const attentionCount = rows.filter(row => row.retryable).length;
  const terminalCount = rows.filter(row => !row.active).length;
  const visibleRows = rows.slice(0, 8);
  return <div className="panel active-work-panel">
    <div className="panel-head"><h2>Active Work</h2><div className="actions"><button onClick={() => setPage('runs')}><TerminalSquare size={14} />Runs</button><button onClick={() => setPage('settings')}><Settings size={14} />Local jobs</button></div></div>
    <div className="mini-grid">
      <span>{activeCount} active</span>
      <span>{attentionCount} need attention</span>
      <span>{terminalCount} finished/recoverable</span>
      <span>{rows.length} tracked</span>
    </div>
    {visibleRows.length ? <table className="compact-table active-work-table"><thead><tr><th>Type</th><th>Item</th><th>Status</th><th>Progress</th><th>Message</th><th>Started</th><th></th></tr></thead><tbody>
      {visibleRows.map(row => <tr key={row.key}><td>{row.kind}</td><td>{row.label}</td><td><span className={`pill ${jobStatusClass(row.status)}`}>{row.status}</span></td><td><WorkProgress percent={row.percent} label={row.progress} /></td><td><span title={row.message}>{row.message || '-'}</span></td><td>{formatDateTime(row.startedAt)}</td><td><div className="row-actions">{row.cancellable ? <button disabled={busy || row.status === 'cancelling'} title={`Stop ${row.kind.toLowerCase()} ${row.shortId}`} onClick={() => onCancel(row)}><Square size={14} />Stop</button> : null}{row.retryable ? <button disabled={busy} title={`Retry ${row.kind.toLowerCase()} ${row.shortId}`} onClick={() => onRetry(row)}><RotateCcw size={14} />Retry</button> : null}<button onClick={() => setPage(row.page)}>{row.page === 'runs' ? 'Open runs' : 'Open local jobs'}</button></div></td></tr>)}
    </tbody></table> : <p className="muted">No queued, running, failed, or recent benchmark work. Start a local/cloud run from Run Builder or download a local model from Settings.</p>}
  </div>;
}

function WorkProgress({ percent, label }: { percent: number | null; label: string }) {
  return <div className="progress-cell">
    <div className="progress-track"><div className="progress-fill" style={{ width: `${percent ?? 0}%` }} /></div>
    <span>{label}</span>
  </div>;
}

function buildActiveWorkRows(runJobs: RunJob[], downloadJobs: HuggingFaceDownloadJob[], serverJobs: HuggingFaceServerJob[]) {
  const rows: ActiveWorkRow[] = [
    ...runJobs.map(job => {
      const percent = job.total > 0 ? Math.min(100, Math.max(0, (job.completed / job.total) * 100)) : null;
      return {
        key: `run-${job.id}`,
        id: job.id,
        shortId: job.id.slice(0, 8),
        workType: 'run' as const,
        kind: 'Run',
        label: `${job.benchmarkPackId} / ${job.id.slice(0, 8)}`,
        status: job.status,
        active: isJobActive(job),
        retryable: isJobRetryable(job),
        cancellable: isJobActive(job),
        progress: job.total > 0 ? `${job.completed}/${job.total}` : `${job.completed}/-`,
        percent,
        message: job.error?.trim() || job.message,
        startedAt: job.startedAt,
        page: 'runs' as Page,
      };
    }),
    ...downloadJobs.map(job => {
      const percent = downloadJobPercent(job);
      return {
        key: `download-${job.id}`,
        id: job.id,
        shortId: job.id.slice(0, 8),
        workType: 'download' as const,
        kind: 'Download',
        label: `${job.repoId}${job.selectedFile ? ` / ${job.selectedFile}` : ''}`,
        status: job.status,
        active: isDownloadJobActive(job),
        retryable: isDownloadJobRetryable(job),
        cancellable: isDownloadJobActive(job),
        progress: percent == null ? formatBytes(job.transferredBytes) : `${percent.toFixed(0)}%`,
        percent,
        message: job.error?.trim() || job.message,
        startedAt: job.startedAt,
        page: 'settings' as Page,
      };
    }),
    ...serverJobs.map(job => ({
      key: `server-${job.id}`,
      id: job.id,
      shortId: job.id.slice(0, 8),
      workType: 'server' as const,
      kind: 'Server',
      label: `${job.repoId}${job.selectedFile ? ` / ${job.selectedFile}` : ''}`,
      status: job.status,
      active: isServerJobActive(job),
      retryable: isServerJobRetryable(job),
      cancellable: isServerJobActive(job),
      progress: `:${job.port}`,
      percent: isServerJobActive(job) ? 50 : job.status === 'completed' ? 100 : null,
      message: job.error?.trim() || job.message,
      startedAt: job.startedAt,
      page: 'settings' as Page,
    })),
  ];
  return rows.sort((a, b) => Number(b.active) - Number(a.active)
    || Number(workNeedsAttention(b)) - Number(workNeedsAttention(a))
    || b.startedAt.localeCompare(a.startedAt));
}

function workNeedsAttention(row: ActiveWorkRow) {
  return row.retryable;
}

function benchmarkWorkspaceNeedsSetup(
  targets: Target[],
  results: RunResult[],
  runJobs: RunJob[],
  downloadJobs: HuggingFaceDownloadJob[],
  serverJobs: HuggingFaceServerJob[],
) {
  const hasUserTarget = targets.some(target => target.kind !== 'mock' && target.id !== 'mock-agent');
  const hasBenchmarkHistory = results.length > 0;
  const hasRecoverableWork = runJobs.length > 0 || downloadJobs.length > 0 || serverJobs.length > 0;
  return !hasUserTarget && !hasBenchmarkHistory && !hasRecoverableWork;
}

function firstUsefulTargetSetupIntent(adapters: Adapter[], checks: DoctorCheck[], packs: BenchmarkPack[]): Omit<TargetSetupIntent, 'nonce'> | null {
  const benchmarkPackId = recommendedComparisonPackId(packs);
  if (dashboardLocalRuntimeCheck(checks).check.status === 'ok') {
    return { code: 'local_runtime_detect', benchmarkPackId, targetIds: [] };
  }
  const cloudAdapterId = preferredCloudSetupAdapterFromDoctorChecks(adapters, checks);
  if (cloudAdapterId) {
    return { adapterId: cloudAdapterId, code: 'missing_key', benchmarkPackId, targetIds: [] };
  }
  return null;
}

function dashboardLocalRuntimeCheck(checks: DoctorCheck[]) {
  const endpointChecks = checks.filter(check => check.id.startsWith('endpoint-'));
  const ready = endpointChecks.filter(check => check.status === 'ok').length;
  const warn = endpointChecks.filter(check => check.status === 'warn').length;
  const error = endpointChecks.filter(check => check.status === 'error').length;
  const check: DoctorCheck = {
    id: 'dashboard-local-runtime-endpoints',
    label: 'Local runtime endpoints',
    status: ready ? 'ok' : error ? 'error' : 'warn',
    detail: endpointChecks.length
      ? ready
        ? `${ready}/${endpointChecks.length} default local endpoint(s) reachable`
        : `${warn + error}/${endpointChecks.length} default local endpoint(s) need attention`
      : 'Doctor has not checked local runtime endpoints yet',
    category: 'Benchmark readiness',
    importance: 'recommended',
    remediation: ready ? 'Open Targets to auto-detect reachable endpoints, or use Local Runtimes > Detect.' : 'Start Ollama, LM Studio, llama.cpp, vLLM, or MLX, then detect local runtimes.',
    command: 'Targets > Local Runtimes',
  };
  return {
    check,
    value: endpointChecks.length ? `${ready}/${endpointChecks.length}` : '-',
    note: ready ? 'ready endpoints' : endpointChecks.length ? 'none reachable' : 'not checked',
  };
}

function dashboardSandboxCheck(checks: DoctorCheck[]) {
  const docker = checks.find(check => check.id === 'docker');
  const colima = checks.find(check => check.id === 'colima');
  const dockerReady = docker?.status === 'ok';
  const colimaReady = colima?.status === 'ok';
  const status: DoctorCheck['status'] = dockerReady ? 'ok' : 'warn';
  const detail = dockerReady
    ? `Docker CLI ready${colimaReady ? '; Colima installed' : ''}`
    : colimaReady
      ? 'Colima is installed; start Colima and ensure Docker CLI is available before Docker scoring'
      : 'Docker/Colima not ready for Docker-backed scoring';
  const check: DoctorCheck = {
    id: 'dashboard-sandbox',
    label: 'Docker/Colima sandbox',
    status,
    detail,
    category: 'Benchmark readiness',
    importance: 'optional',
    remediation: dockerReady ? 'Enable Docker scoring in Runs for eligible Python repo/code tasks.' : 'Install Docker Desktop or Colima, then rerun Doctor.',
    command: 'Runs > Docker scoring',
  };
  return {
    check,
    value: dockerReady ? 'Ready' : 'Optional',
    note: dockerReady ? 'Docker scoring available' : colimaReady ? 'Colima installed' : 'host scoring only',
  };
}

function dashboardRecentCost(results: RunResult[], days: number): DashboardRecentCost {
  const cutoff = Date.now() - days * 24 * 60 * 60 * 1000;
  let rows = 0;
  let pricedRows = 0;
  let costUsd = 0;
  for (const result of results) {
    const started = parseResultStartedAt(result.started_at);
    if (started == null || started < cutoff) {
      continue;
    }
    rows += 1;
    const rowCostUsd = resultCostUsdForCoverage(result);
    if (rowCostUsd != null) {
      pricedRows += 1;
      costUsd += rowCostUsd;
    }
  }
  return { rows, pricedRows, costUsd: pricedRows ? costUsd : null };
}

function compactDashboardValue(value: string) {
  return value.length > 18 ? `${value.slice(0, 18)}...` : value;
}

function dashboardResultIsModelSelectionResult(result: RunResult, targetById: Map<string, Target>) {
  if (!resultPackId(result).startsWith('llm-')) {
    return false;
  }
  const target = targetById.get(resultTargetId(result));
  if (target) {
    return dashboardTargetIsModelSelectionTarget(target);
  }
  const reproTarget = resultReproducibilityTarget(result);
  if (reproTarget) {
    return resultReproducibilityTargetIsModelSelectionTarget(reproTarget);
  }
  const adapterId = resultAdapterId(result);
  return Boolean(adapterId && (localModelAdapters.has(adapterId) || cloudModelAdapters.has(adapterId)));
}

function resultReproducibilityTarget(result: RunResult): Record<string, unknown> | null {
  const target = result.reproducibility?.target;
  return isRecord(target) ? target : null;
}

function resultGenerationSamplingFingerprint(result: RunResult) {
  const generationValue = result.reproducibility?.generation;
  const generation = isRecord(generationValue) ? generationValue : null;
  const temperature = canonicalGenerationValue(generation?.temperature) ?? 'not_reported';
  const topP = canonicalGenerationValue(generation?.top_p) ?? 'not_reported';
  const seed = canonicalGenerationValue(generation?.seed) ?? 'not_set';
  const mode = generationSamplingMode(generation);
  return `mode ${mode}, temp ${temperature}, top_p ${topP}, seed ${seed}`;
}

function canonicalGenerationValue(value: unknown): string | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    if (Math.abs(value - Math.round(value)) < 0.000001) {
      return String(Math.round(value));
    }
    let formatted = value.toFixed(4);
    while (formatted.includes('.') && formatted.endsWith('0')) {
      formatted = formatted.slice(0, -1);
    }
    return formatted.endsWith('.') ? formatted.slice(0, -1) : formatted;
  }
  if (typeof value === 'string' && value.trim()) {
    return value;
  }
  if (typeof value === 'boolean') {
    return String(value);
  }
  return null;
}

function generationSamplingMode(generation: Record<string, unknown> | null) {
  const temperature = typeof generation?.temperature === 'number' ? generation.temperature : null;
  const topP = typeof generation?.top_p === 'number' ? generation.top_p : null;
  if (temperature != null && topP != null && Math.abs(temperature) <= 0.000001 && topP >= 0.999999) {
    return 'deterministic';
  }
  if (temperature != null && temperature > 0.000001) {
    return 'exploratory';
  }
  if (topP != null && topP < 0.999999) {
    return 'exploratory';
  }
  return 'unknown';
}

function dashboardTargetIsModelSelectionTarget(target: Target) {
  return (target.kind === 'direct_model' || target.kind === 'harnessed_model')
    && (isLocalModelTarget(target) || isCloudModelTarget(target));
}

function resultReproducibilityTargetIsModelSelectionTarget(target: Record<string, unknown>) {
  const kind = stringValue(target.kind);
  if (kind !== 'direct_model' && kind !== 'harnessed_model') {
    return false;
  }
  const config = isRecord(target.config) ? target.config : {};
  return resultReproducibilityTargetLooksLocal(target, config)
    || resultReproducibilityTargetLooksCloud(target, config);
}

function resultReproducibilityTargetLooksLocal(target: Record<string, unknown>, config: Record<string, unknown>) {
  if (stringValue(config.source) === 'huggingface-local') {
    return true;
  }
  const baseUrl = stringValue(config.base_url);
  const adapterId = stringValue(target.adapter_id) || stringValue(target.adapterId);
  if (baseUrl && dashboardBaseUrlLooksRemote(baseUrl)) {
    return false;
  }
  if (baseUrl && dashboardBaseUrlLooksLocal(baseUrl)) {
    return !cloudModelAdapters.has(adapterId);
  }
  return localModelAdapters.has(adapterId) || adapterId === 'openai-compatible' || adapterId === 'generic-openai-compatible';
}

function resultReproducibilityTargetLooksCloud(target: Record<string, unknown>, config: Record<string, unknown>) {
  const baseUrl = stringValue(config.base_url);
  const adapterId = stringValue(target.adapter_id) || stringValue(target.adapterId);
  if (baseUrl && dashboardBaseUrlLooksRemote(baseUrl)) {
    return true;
  }
  return cloudModelAdapters.has(adapterId);
}

function resultCostUsdForCoverage(result: RunResult): number | null {
  if (typeof result.cost_usd === 'number' && Number.isFinite(result.cost_usd)) {
    return result.cost_usd;
  }
  return resultKnownZeroCostWhenUnpriced(result) ? 0 : null;
}

function resultHasCostCoverage(result: RunResult) {
  return resultCostUsdForCoverage(result) != null;
}

function resultKnownZeroCostWhenUnpriced(result: RunResult) {
  if (typeof result.cost_usd === 'number' && Number.isFinite(result.cost_usd)) {
    return false;
  }
  const target = resultReproducibilityTarget(result);
  if (!target) {
    return false;
  }
  const kind = stringValue(target.kind) || 'direct_model';
  const adapterId = stringValue(target.adapter_id) || stringValue(target.adapterId);
  const config = isRecord(target.config) ? target.config : {};
  if (kind === 'mock') {
    return true;
  }
  if (kind !== 'direct_model' && kind !== 'harnessed_model') {
    return false;
  }
  if (stringValue(config.source) === 'huggingface-local') {
    return true;
  }
  const baseUrl = stringValue(config.base_url);
  if (baseUrl && dashboardBaseUrlLooksRemote(baseUrl)) {
    return false;
  }
  if (baseUrl && dashboardBaseUrlLooksLocal(baseUrl)) {
    return !cloudModelAdapters.has(adapterId);
  }
  return localModelAdapters.has(adapterId) || adapterId === 'openai-compatible' || adapterId === 'generic-openai-compatible';
}

function dashboardBaseUrlLooksLocal(baseUrl: string) {
  const lower = baseUrl.toLowerCase();
  return lower.includes('://localhost') || lower.includes('://127.0.0.1') || lower.includes('://0.0.0.0');
}

function dashboardBaseUrlLooksRemote(baseUrl: string) {
  return baseUrl.startsWith('http') && !dashboardBaseUrlLooksLocal(baseUrl);
}

function stringValue(value: unknown) {
  return typeof value === 'string' ? value.trim() : '';
}

function cloudSetupAdapterCandidates(adapters: Adapter[]) {
  const adapterById = new Map(adapters.map(adapter => [adapter.id, adapter]));
  const preferred = preferredCloudSetupAdapterIds
    .map(id => adapterById.get(id))
    .filter((adapter): adapter is Adapter => Boolean(adapter));
  const preferredIds = new Set(preferred.map(adapter => adapter.id));
  const remaining = adapters.filter(adapter => !preferredIds.has(adapter.id) && adapterLooksLikeCloudSetup(adapter));
  return [...preferred, ...remaining];
}

function adapterLooksLikeCloudSetup(adapter: Adapter) {
  return ['openai_compatible', 'openai_responses', 'anthropic_messages', 'mistral_api', 'azure_openai'].includes(adapter.kind)
    || cloudModelAdapters.has(adapter.id);
}

function preferredCloudSetupAdapterFromDoctorChecks(adapters: Adapter[], checks: DoctorCheck[]) {
  const readyAdapterIds = new Set(
    checks
      .filter(check => check.status === 'ok')
      .map(cloudKeyDoctorAdapterId)
      .filter(Boolean),
  );
  if (!readyAdapterIds.size) {
    return '';
  }
  return cloudSetupAdapterCandidates(adapters).find(adapter => readyAdapterIds.has(adapter.id))?.id ?? '';
}

function preferredCloudSetupAdapterId(adapters: Adapter[], readyAdapterId = '') {
  const availableIds = new Set(adapters.map(adapter => adapter.id));
  if (readyAdapterId && availableIds.has(readyAdapterId)) {
    return readyAdapterId;
  }
  return cloudSetupAdapterCandidates(adapters)[0]?.id ?? 'openrouter';
}

function usePreferredCloudSetupAdapterId(adapters: Adapter[], checks?: DoctorCheck[]) {
  const doctorChecks = checks ?? emptyDoctorChecks;
  const doctorPreferredAdapterId = useMemo(
    () => preferredCloudSetupAdapterFromDoctorChecks(adapters, doctorChecks),
    [adapters, doctorChecks],
  );
  const [statusPreferredAdapterId, setStatusPreferredAdapterId] = useState('');

  useEffect(() => {
    if (doctorPreferredAdapterId) {
      setStatusPreferredAdapterId('');
      return;
    }
    let cancelled = false;
    const candidates = cloudSetupAdapterCandidates(adapters)
      .filter(adapter => adapterNeedsApiKey(adapter, adapter.defaultBaseUrl ?? ''));
    setStatusPreferredAdapterId('');
    if (!candidates.length) {
      return;
    }
    Promise.all(candidates.map(async adapter => {
      try {
        const status = await providerApiKeyStatus(providerKeychainId(adapter, adapter.defaultBaseUrl ?? ''));
        return status.available ? adapter.id : '';
      } catch {
        return '';
      }
    })).then(adapterIds => {
      if (!cancelled) {
        setStatusPreferredAdapterId(adapterIds.find(Boolean) ?? '');
      }
    });
    return () => {
      cancelled = true;
    };
  }, [adapters, doctorPreferredAdapterId]);

  return preferredCloudSetupAdapterId(adapters, doctorPreferredAdapterId || statusPreferredAdapterId);
}

function dashboardCheck(checks: DoctorCheck[], id: string, label: string, status: DoctorCheck['status'], detail: string): DoctorCheck {
  return checks.find(check => check.id === id) ?? {
    id,
    label,
    status,
    detail,
    category: 'Benchmark readiness',
    importance: 'recommended',
    remediation: '',
    command: '',
  };
}

interface DashboardLocalCloudComparisonTargets {
  runTargetIds: string[];
  allRunTargetIds: string[];
  setupLocalTargetIds: string[];
  setupCloudTargetIds: string[];
  pricingRepairTargetIds: string[];
  skippedUnpricedCloudTargetIds: string[];
}

function dashboardLocalCloudComparisonTargets(targets: Target[]): DashboardLocalCloudComparisonTargets {
  const selectable = targets.filter(targetIsSelectableModel);
  const localTargets = selectable.filter(dashboardTargetLooksLocal).sort(compareDashboardComparisonTargetPriority);
  const localIds = localTargets.map(target => target.id);
  const cloudTargets = selectable.filter(dashboardTargetLooksCloud);
  const pricedCloudTargets = cloudTargets.filter(targetHasInputOutputPricing).sort(compareDashboardComparisonTargetPriority);
  const pricedCloudIds = pricedCloudTargets.map(target => target.id);
  const unpricedCloudIds = cloudTargets
    .filter(target => !targetHasInputOutputPricing(target))
    .map(target => target.id);
  const pricingRepairTargetIds = localIds.length && !pricedCloudIds.length
    ? unpricedCloudIds
    : [];
  const recommendedPairIds = localTargets.length && pricedCloudTargets.length
    ? [localTargets[0].id, pricedCloudTargets[0].id]
    : [];
  return {
    runTargetIds: uniqueIdsInOrder(recommendedPairIds),
    allRunTargetIds: localIds.length && pricedCloudIds.length ? uniqueIdsInOrder([...localIds, ...pricedCloudIds]) : [],
    setupLocalTargetIds: localIds[0] ? [localIds[0]] : [],
    setupCloudTargetIds: pricedCloudIds[0] ? [pricedCloudIds[0]] : [],
    pricingRepairTargetIds: uniqueIdsInOrder(pricingRepairTargetIds),
    skippedUnpricedCloudTargetIds: localIds.length && pricedCloudIds.length ? uniqueIdsInOrder(unpricedCloudIds) : [],
  };
}

function dashboardPrimaryComparisonTargetIds(recommendedTargetIds: string[], allComparableTargetIds: string[]) {
  if (allComparableTargetIds.length >= 2 && allComparableTargetIds.length <= dashboardPrimaryComparisonTargetLimit) {
    return allComparableTargetIds;
  }
  return recommendedTargetIds;
}

function dashboardPrimaryComparisonActionLabel(primaryTargetIds: string[], allComparableTargetIds: string[]) {
  if (primaryTargetIds.length >= 2 && primaryTargetIds.length === allComparableTargetIds.length) {
    return `Compare ${primaryTargetIds.length} models`;
  }
  if (primaryTargetIds.length >= 2) {
    return 'Compare recommended pair';
  }
  return 'Run model comparison';
}

function compareDashboardComparisonTargetPriority(left: Target, right: Target) {
  return dashboardComparisonTargetReadinessRank(left) - dashboardComparisonTargetReadinessRank(right)
    || left.name.localeCompare(right.name)
    || left.id.localeCompare(right.id);
}

function dashboardComparisonTargetReadinessRank(target: Target) {
  if (target.validationStatus === 'ok') {
    return 0;
  }
  if (target.validationStatus === 'warn') {
    return 1;
  }
  if (target.validationStatus) {
    return 2;
  }
  return 3;
}

function uniqueIdsInOrder(values: string[]) {
  return Array.from(new Set(values));
}

function dashboardTargetLooksLocal(target: Target) {
  return isLocalModelTarget(target)
    || localModelAdapters.has(target.adapterId)
    || target.id.includes('local')
    || target.name.toLowerCase().includes('local');
}

function dashboardTargetLooksCloud(target: Target) {
  return isCloudModelTarget(target)
    || cloudModelAdapters.has(target.adapterId)
    || target.id.includes('cloud')
    || target.name.toLowerCase().includes('cloud');
}

function recordAppDiagnostic(kind: string, error: unknown) {
  recordDiagnosticEvent({
    kind,
    level: 'error',
    message: diagnosticMessage(error),
    detail: diagnosticDetail(error),
  }).catch(() => undefined);
}

function diagnosticMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message || error.name;
  }
  return String(error);
}

function diagnosticDetail(error: unknown) {
  if (error instanceof Error) {
    return error.stack || error.message;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function benchmarkRunFailureMessage(error: unknown) {
  const detail = diagnosticMessage(error).replace(/^Error:\s*/, '').trim();
  if (!detail) {
    return 'Benchmark run failed. Review target validation, pricing, and run settings before retrying.';
  }
  if (detail.startsWith('max_cost_unpriced')) {
    return `${detail} Add input/output pricing to the listed cloud target(s), choose only known-zero local targets, or clear Max cost USD if you intentionally want an uncapped run.`;
  }
  if (detail.startsWith('max_cost_exceeded')) {
    return `${detail} Lower repetitions, warmups, target count, or max tokens, or raise Max cost USD before retrying.`;
  }
  if (detail.startsWith('target_preflight_failed') || detail.includes('target_validation_failed')) {
    return `${detail} Revalidate the target after fixing the endpoint, key, model name, or local runtime, or edit the target before retrying.`;
  }
  if (detail.startsWith('target_not_found')) {
    return `${detail} Refresh Targets and rebuild the run selection before retrying.`;
  }
  if (detail.startsWith('incompatible_target')) {
    return `${detail} Choose a benchmark pack that supports the selected target kind, or change the selected targets.`;
  }
  if (detail.startsWith('docker_unavailable')) {
    return `${detail} Start Docker Desktop or Colima, or disable Docker scoring for this run.`;
  }
  if (detail.startsWith('active_job_replay_blocked')) {
    return `${detail} Wait for the current job to finish, or cancel it before retrying or duplicating.`;
  }
  return detail;
}

function adapterNeedsApiKey(adapter?: Adapter, configuredBaseUrl = '') {
  if (!adapter) {
    return false;
  }
  return ['openai_responses', 'anthropic_messages', 'mistral_api', 'azure_openai'].includes(adapter.kind)
    || cloudModelAdapters.has(adapter.id)
    || (adapter.id === 'openai-compatible' && dashboardBaseUrlLooksRemote(configuredBaseUrl.trim()));
}

function providerKeychainId(adapter: Adapter, configuredBaseUrl = '') {
  const normalizedBaseUrl = configuredBaseUrl.trim().replace(/\/+$/, '');
  if (adapter.id === 'openai-compatible' && dashboardBaseUrlLooksRemote(normalizedBaseUrl)) {
    return slugify(`openai-compatible-${normalizedBaseUrl}`);
  }
  return adapter.id;
}

function targetFormBaseUrlIsPrimary(adapter?: Adapter) {
  if (!adapter) {
    return false;
  }
  if (adapter.kind === 'azure_openai' || adapter.id === 'openai-compatible') {
    return true;
  }
  if (!adapter.defaultBaseUrl) {
    return true;
  }
  return localModelAdapters.has(adapter.id) || dashboardBaseUrlLooksLocal(adapter.defaultBaseUrl);
}

function providerKeyStatus(needsApiKey: boolean, checking: boolean, available: boolean, pendingKey: string, pendingEnv: string, detail: string) {
  if (!needsApiKey) {
    return { label: 'optional', className: 'ok', detail: 'local or compatible endpoint can run without a stored cloud key' };
  }
  if (pendingKey.trim()) {
    return { label: 'pending', className: 'warn', detail: 'key will be saved to Keychain before search or save' };
  }
  if (pendingEnv.trim()) {
    return { label: 'env ref', className: 'warn', detail: `${pendingEnv.trim()} will be read from the app environment` };
  }
  if (checking) {
    return { label: 'checking', className: 'warn', detail: 'checking Keychain and environment' };
  }
  if (available) {
    return { label: 'available', className: 'ok', detail: detail || 'Keychain or environment key is available' };
  }
  return { label: 'missing', className: 'error', detail: detail || 'paste a key before searching or adding this cloud target' };
}

function AutomaticBenchmarkPreview({
  enabled,
  plannedTarget,
  intent,
  targets,
  packLabel,
  needsPricing,
  unpricedCloudTargetIds,
}: {
  enabled: boolean;
  plannedTarget: Target | null;
  intent: RunBuilderIntent | null;
  targets: Target[];
  packLabel: string;
  needsPricing: boolean;
  unpricedCloudTargetIds: string[];
}) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  const targetNames = intent?.targetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById)) ?? [];
  const unpricedCloudNames = unpricedCloudTargetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById));
  const unpricedExistingCloudNames = targets
    .filter(target => target.id !== plannedTarget?.id && isCloudModelTarget(target) && !targetHasInputOutputPricing(target))
    .map(target => target.name || target.id);
  const tone = !enabled ? '' : needsPricing || !plannedTarget ? 'warn' : 'ok';
  const headline = !enabled
    ? 'Automatic benchmark off'
    : !plannedTarget
      ? 'Choose a model to preview the automatic handoff'
      : needsPricing
        ? 'Automatic benchmark waits for pricing'
        : intent && intent.targetIds.length > 1
          ? 'Ready to compare after add'
          : 'Ready to run after add';
  const detail = !enabled
    ? 'BenchForge will save and validate the target only.'
    : !plannedTarget
      ? 'Enter a model or choose a catalog row; BenchForge will show whether the target can run by itself or compare with an existing counterpart.'
      : needsPricing
        ? `Add input/output pricing for ${previewList(unpricedCloudNames)} before BenchForge can queue a capped run automatically.`
        : automaticBenchmarkPreviewDetail(plannedTarget, intent, unpricedExistingCloudNames);
  return <div className={`preflight-box auto-benchmark-preview span-two ${tone}`}>
    <div className="panel-head"><h2>Automatic benchmark</h2><span className={`pill ${tone || 'unknown'}`}>{enabled ? (intent && intent.targetIds.length > 1 ? 'compare' : 'run') : 'off'}</span></div>
    <p><strong>{headline}</strong></p>
    <p>{detail}</p>
    {enabled && intent ? <div className="mini-grid">
      <span>{packLabel}</span>
      <span>{targetNames.length ? previewList(targetNames, 4) : '-'}</span>
      <span>{intent.repetitions ?? 1} rep / {intent.warmupRuns ?? 0} warmup</span>
      <span>{typeof intent.maxCostUsd === 'number' ? `${formatCost(intent.maxCostUsd)} cap` : 'no cost cap'}</span>
    </div> : null}
  </div>;
}

function AutomaticBenchmarkInlinePreview({
  enabled,
  plannedTarget,
  intent,
  targets,
  packLabel,
  needsPricing,
  unpricedCloudTargetIds,
}: {
  enabled: boolean;
  plannedTarget: Target | null;
  intent: RunBuilderIntent | null;
  targets: Target[];
  packLabel: string;
  needsPricing: boolean;
  unpricedCloudTargetIds: string[];
}) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  const targetNames = intent?.targetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById)) ?? [];
  const unpricedCloudNames = unpricedCloudTargetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById));
  const unpricedExistingCloudNames = targets
    .filter(target => target.id !== plannedTarget?.id && isCloudModelTarget(target) && !targetHasInputOutputPricing(target))
    .map(target => target.name || target.id);
  const tone = !enabled ? 'unknown' : needsPricing || !plannedTarget || !intent ? 'warn' : 'ok';
  const label = !enabled
    ? 'save only'
    : !plannedTarget
      ? 'needs model'
      : needsPricing
        ? 'pricing'
        : intent && intent.targetIds.length > 1
          ? 'compare'
          : 'run';
  const runShape = intent ? `${intent.repetitions ?? 1} rep / ${intent.warmupRuns ?? 0} warmup` : '';
  const detail = !enabled
    ? 'Automatic benchmark is off.'
    : !plannedTarget
      ? 'Start the runtime and choose a model.'
      : needsPricing
        ? `Add pricing for ${previewList(unpricedCloudNames)}.`
        : intent && intent.targetIds.length > 1
          ? `${packLabel}; ${targetNames.length} targets; ${runShape}.`
          : unpricedExistingCloudNames.length
            ? `${packLabel}; run local only. Price ${previewList(unpricedExistingCloudNames, 2)} to compare.`
            : `${packLabel}; run this target; ${runShape}.`;
  return <div className={`handoff-inline-preview ${tone}`} title={targetNames.length ? previewList(targetNames, 6) : detail}>
    <span className={`pill ${tone}`}>{label}</span>
    <span>{detail}</span>
  </div>;
}

function PendingAutomaticBenchmarkPanel({
  plannedTarget,
  intent,
  targets,
  packLabel,
  needsPricing,
  unpricedCloudTargetIds,
}: {
  plannedTarget: Target | null;
  intent: RunBuilderIntent | null;
  targets: Target[];
  packLabel: string;
  needsPricing: boolean;
  unpricedCloudTargetIds: string[];
}) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  const targetNames = intent?.targetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById)) ?? [];
  const unpricedCloudNames = unpricedCloudTargetIds.map(id => automaticPreviewTargetLabel(id, plannedTarget, targetById));
  const unpricedCloudLabel = previewList(unpricedCloudNames) || 'the selected cloud target';
  const tone = needsPricing || !plannedTarget || !intent ? 'warn' : 'ok';
  const headline = !plannedTarget || !intent
    ? 'Fix target details to continue automatic benchmark'
    : needsPricing
    ? 'Add pricing to continue automatic benchmark'
    : intent.targetIds.length > 1
      ? 'Ready to compare after update'
      : 'Ready to run after update';
  const detail = !plannedTarget || !intent
    ? 'Resolve the target fields above so BenchForge can rebuild the pending benchmark plan.'
    : needsPricing
      ? `Enter both input and output pricing for ${unpricedCloudLabel} and update the target. BenchForge will validate it, then queue the pending capped benchmark.`
      : `Update the target to validate it and queue the pending ${packLabel} ${intent.targetIds.length > 1 ? 'local/cloud comparison' : 'benchmark'}.`;
  return <div className={`preflight-box auto-benchmark-preview span-two ${tone}`}>
    <div className="panel-head"><h2>Pending automatic benchmark</h2><span className={`pill ${tone}`}>{needsPricing ? 'pricing' : 'ready'}</span></div>
    <p><strong>{headline}</strong></p>
    <p>{detail}</p>
    {intent ? <div className="mini-grid">
      <span>{packLabel}</span>
      <span>{targetNames.length ? previewList(targetNames, 4) : '-'}</span>
      <span>{intent.repetitions ?? 1} rep / {intent.warmupRuns ?? 0} warmup</span>
      <span>{typeof intent.maxCostUsd === 'number' ? `${formatCost(intent.maxCostUsd)} cap` : 'no cost cap'}</span>
    </div> : null}
  </div>;
}

function TargetSetupPlanPanel({
  benchmarkPackId,
  packLabel,
  targetIds,
  targets,
}: {
  benchmarkPackId: string;
  packLabel: string;
  targetIds: string[];
  targets: Target[];
}) {
  const settings = automaticModelBenchmarkSettings(benchmarkPackId);
  const targetNames = targetLabelsById(targetIds, targets);
  return <div className="preflight-box setup-plan-box span-two ok">
    <div className="panel-head"><h2>Automatic setup plan</h2><span className="pill ok">compare</span></div>
    <p><strong>New target will join the selected comparison.</strong></p>
    <p>After the target is saved and validated, BenchForge will run the same benchmark pack with the new target plus {previewList(targetNames)}.</p>
    <div className="mini-grid">
      <span>{packLabel}</span>
      <span>{previewList(targetNames, 4)}</span>
      <span>{settings.repetitions} rep / {settings.warmupRuns} warmup</span>
      <span>{formatCost(automaticModelBenchmarkMaxCostUsd(benchmarkPackId))} cap</span>
    </div>
  </div>;
}

function automaticPreviewTargetLabel(id: string, plannedTarget: Target | null, targetById: Map<string, Target>) {
  if (plannedTarget && id === plannedTarget.id) {
    return `new: ${plannedTarget.name}`;
  }
  const target = targetById.get(id);
  return target?.name || target?.id || id;
}

function automaticBenchmarkPreviewDetail(plannedTarget: Target, intent: RunBuilderIntent | null, unpricedExistingCloudNames: string[]) {
  if (intent && intent.targetIds.length > 1) {
    return 'BenchForge will save the target, validate it, then queue the same benchmark pack across the listed local/cloud targets.';
  }
  if (isLocalModelTarget(plannedTarget)) {
    if (unpricedExistingCloudNames.length) {
      return `BenchForge will run this local target by itself. Add pricing for ${previewList(unpricedExistingCloudNames)} to make the next add a capped local/cloud comparison.`;
    }
    return 'BenchForge will run this local target by itself. Add a priced cloud target when you want automatic local/cloud comparison.';
  }
  if (isCloudModelTarget(plannedTarget)) {
    return 'BenchForge will run this cloud target by itself. Add a local runtime or Hugging Face model when you want automatic local/cloud comparison.';
  }
  return 'BenchForge will save, validate, and run this target with the selected benchmark pack.';
}

function adapterModelPresets(adapter?: Adapter): ModelPreset[] {
  const raw = adapter?.metadata?.model_presets;
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw.flatMap((item, index) => {
    if (!item || typeof item !== 'object') {
      return [];
    }
    const data = item as Record<string, unknown>;
    const model = typeof data.model === 'string' ? data.model : '';
    if (!model) {
      return [];
    }
    const label = typeof data.label === 'string' ? data.label : model;
    return [{
      id: `${model}-${index}`,
      label,
      model,
      inputPrice: numberFromUnknown(data.input_price_usd_per_million_tokens),
      outputPrice: numberFromUnknown(data.output_price_usd_per_million_tokens),
      cacheReadPrice: numberFromUnknown(data.cache_read_price_usd_per_million_tokens ?? data.cached_input_price_usd_per_million_tokens),
      cacheWritePrice: numberFromUnknown(data.cache_write_price_usd_per_million_tokens ?? data.cache_creation_price_usd_per_million_tokens),
      contextLength: numberFromUnknown(data.context_length ?? data.max_context_length),
      source: typeof data.source === 'string' ? data.source : undefined,
      note: typeof data.note === 'string' ? data.note : undefined,
    }];
  });
}

function adapterPresetCloudModels(adapter?: Adapter): CloudModel[] {
  return adapterModelPresets(adapter).map(preset => ({
    model: preset.model,
    name: preset.label,
    provider: adapter?.name ?? 'Cloud provider',
    inputPriceUsdPerMillionTokens: preset.inputPrice ?? null,
    outputPriceUsdPerMillionTokens: preset.outputPrice ?? null,
    cacheReadPriceUsdPerMillionTokens: preset.cacheReadPrice ?? null,
    cacheWritePriceUsdPerMillionTokens: preset.cacheWritePrice ?? null,
    contextLength: preset.contextLength ?? null,
    source: 'adapter-preset',
    sourceUrl: preset.source,
    detail: preset.note,
  }));
}

function matchingPricedModelPreset(adapter: Adapter | undefined, modelValue: string) {
  const normalizedModel = modelValue.trim().toLowerCase();
  if (!normalizedModel) {
    return undefined;
  }
  return adapterModelPresets(adapter).find(preset => {
    if (preset.model.trim().toLowerCase() !== normalizedModel) {
      return false;
    }
    return preset.inputPrice != null
      || preset.outputPrice != null
      || preset.cacheReadPrice != null
      || preset.cacheWritePrice != null;
  });
}

function numberFromUnknown(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function formValueFromUnknown(value: unknown, fallback = '') {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return String(value);
  }
  if (typeof value === 'string' && value !== '[REDACTED]') {
    return value;
  }
  return fallback;
}

function Targets({ targets, adapters, packs, checks, onRefresh, setMessage, openRunBuilder, openResultsForGroup, repairIntent, onRepairIntentConsumed, setupIntent, onSetupIntentConsumed }: { targets: Target[]; adapters: Adapter[]; packs: BenchmarkPack[]; checks: DoctorCheck[]; onRefresh: () => Promise<void>; setMessage: (message: string) => void; openRunBuilder: (intent: RunBuilderIntent) => void; openResultsForGroup: (groupId: string, runId?: string) => void; repairIntent: TargetRepairIntent | null; onRepairIntentConsumed: () => void; setupIntent: TargetSetupIntent | null; onSetupIntentConsumed: () => void }) {
  const runnableAdapters = adapters.filter(adapter => ['openai_compatible', 'openai_responses', 'anthropic_messages', 'mistral_api', 'azure_openai'].includes(adapter.kind));
  const preferredDirectSetupAdapterId = usePreferredCloudSetupAdapterId(adapters, checks);
  const [adapterId, setAdapterId] = useState('');
  const [adapterAutoSelected, setAdapterAutoSelected] = useState(false);
  const [modelPresetId, setModelPresetId] = useState('custom');
  const [targetName, setTargetName] = useState('');
  const [model, setModel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [apiKeyEnv, setApiKeyEnv] = useState('');
  const [temperature, setTemperature] = useState('0');
  const [topP, setTopP] = useState('1');
  const [maxTokens, setMaxTokens] = useState('512');
  const [seed, setSeed] = useState('');
  const [timeoutSeconds, setTimeoutSeconds] = useState('120');
  const [retryCount, setRetryCount] = useState('1');
  const [inputPrice, setInputPrice] = useState('');
  const [outputPrice, setOutputPrice] = useState('');
  const [cacheReadPrice, setCacheReadPrice] = useState('');
  const [cacheWritePrice, setCacheWritePrice] = useState('');
  const [autoBenchmarkAfterAdd, setAutoBenchmarkAfterAdd] = useState(true);
  const [autoBenchmarkPackId, setAutoBenchmarkPackId] = useState(defaultModelComparisonPackId);
  const [autoBenchmarkTargetIds, setAutoBenchmarkTargetIds] = useState<string[]>([]);
  const [comparisonPackId, setComparisonPackId] = useState(defaultModelComparisonPackId);
  const [cloudModelQuery, setCloudModelQuery] = useState('');
  const [cloudModels, setCloudModels] = useState<CloudModel[]>([]);
  const [cloudModelBusy, setCloudModelBusy] = useState(false);
  const [catalogAddBusyModel, setCatalogAddBusyModel] = useState('');
  const [selectedCloudModel, setSelectedCloudModel] = useState<CloudModel | null>(null);
  const [targetAdvancedOpen, setTargetAdvancedOpen] = useState(false);
  const [providerKeyAvailable, setProviderKeyAvailable] = useState(false);
  const [providerKeyDetail, setProviderKeyDetail] = useState('');
  const [providerKeyStatusBusy, setProviderKeyStatusBusy] = useState(false);
  const [localRuntimes, setLocalRuntimes] = useState<LocalRuntime[]>([]);
  const [localSelections, setLocalSelections] = useState<Record<string, string>>({});
  const [detectingLocal, setDetectingLocal] = useState(false);
  const [addingLocalRuntimeId, setAddingLocalRuntimeId] = useState('');
  const [runtimeToolBusy, setRuntimeToolBusy] = useState('');
  const [runtimeToolResult, setRuntimeToolResult] = useState<LocalRuntimeToolResult | null>(null);
  const [validations, setValidations] = useState<Record<string, TargetValidation>>({});
  const [validating, setValidating] = useState('');
  const [editingTargetId, setEditingTargetId] = useState('');
  const [editingTargetHadValidationError, setEditingTargetHadValidationError] = useState(false);
  const [editingTargetPreserveApiKeyRef, setEditingTargetPreserveApiKeyRef] = useState(false);
  const [editingTargetPreserveApiKeyEnvRef, setEditingTargetPreserveApiKeyEnvRef] = useState(false);
  const [editingTargetPendingAutoBenchmarkPackId, setEditingTargetPendingAutoBenchmarkPackId] = useState('');
  const [harnessPresetId, setHarnessPresetId] = useState('custom');
  const [harnessTargetId, setHarnessTargetId] = useState('benchforge-worker');
  const [harnessTargetName, setHarnessTargetName] = useState('BenchForge Worker');
  const [harnessCommand, setHarnessCommand] = useState('');
  const [harnessModel, setHarnessModel] = useState('');
  const [harnessBaseUrl, setHarnessBaseUrl] = useState('');
  const [harnessTimeoutSeconds, setHarnessTimeoutSeconds] = useState('3600');
  const [harnessEnvPassthrough, setHarnessEnvPassthrough] = useState('');
  const [harnessSetupHint, setHarnessSetupHint] = useState('');
  const [harnessToolBusy, setHarnessToolBusy] = useState('');
  const [harnessToolResult, setHarnessToolResult] = useState<HarnessToolResult | null>(null);
  const [editingHarnessTargetId, setEditingHarnessTargetId] = useState('');
  const [loadingTargetId, setLoadingTargetId] = useState('');
  const [duplicatingTargetId, setDuplicatingTargetId] = useState('');
  const apiKeyInputRef = useRef<HTMLInputElement>(null);
  const autoLocalRuntimeDetectAttemptedRef = useRef(false);

  const selectedAdapter = runnableAdapters.find(adapter => adapter.id === adapterId);
  const modelPresets = useMemo(() => adapterModelPresets(selectedAdapter), [selectedAdapter]);
  const selectedModelPreset = modelPresets.find(preset => preset.id === modelPresetId);
  const needsApiKey = adapterNeedsApiKey(selectedAdapter, baseUrl);
  const baseUrlIsPrimary = targetFormBaseUrlIsPrimary(selectedAdapter);
  const selectedProviderKeychainId = selectedAdapter ? providerKeychainId(selectedAdapter, baseUrl) : '';
  const editingTarget = targets.find(target => target.id === editingTargetId);
  const editingHarnessTarget = targets.find(target => target.id === editingHarnessTargetId);
  const selectedHarnessPreset = harnessPresets.find(preset => preset.id === harnessPresetId);
  const modelBenchmarkPacks = useMemo(() => modelBenchmarkPackOptions(packs), [packs]);
  const localRuntimeDoctorCheck = useMemo(() => dashboardLocalRuntimeCheck(checks), [checks]);
  const enabledModelTargets = targets.filter(targetIsSelectableModel);
  const localComparisonTargetIds = enabledModelTargets.filter(isLocalModelTarget).map(target => target.id);
  const cloudComparisonTargets = enabledModelTargets.filter(isCloudModelTarget);
  const pricedCloudComparisonTargets = cloudComparisonTargets.filter(targetHasInputOutputPricing);
  const pricedCloudComparisonTargetIds = pricedCloudComparisonTargets.map(target => target.id);
  const unpricedCloudComparisonTargets = cloudComparisonTargets.filter(target => !targetHasInputOutputPricing(target));
  const skippedUnpricedCloudTargetIds = unpricedCloudComparisonTargets.map(target => target.id);
  const allComparisonTargetIds = localComparisonTargetIds.length && pricedCloudComparisonTargetIds.length
    ? [...localComparisonTargetIds, ...pricedCloudComparisonTargetIds]
    : [];
  const comparisonNeedsCloudPricing = Boolean(localComparisonTargetIds.length && cloudComparisonTargets.length && !pricedCloudComparisonTargetIds.length);
  const comparisonActionDisabled = !localComparisonTargetIds.length || !cloudComparisonTargets.length;

  useEffect(() => {
    if (setupIntent || editingTargetId || editingHarnessTargetId) {
      return;
    }
    const preferred = runnableAdapters.find(adapter => adapter.id === preferredDirectSetupAdapterId);
    const nextAdapterId = preferred?.id ?? runnableAdapters[0]?.id ?? '';
    if (!nextAdapterId) {
      return;
    }
    if (!adapterId || (adapterAutoSelected && adapterId !== nextAdapterId)) {
      selectAdapter(nextAdapterId, { prefillFirstPreset: true, autoSelected: true });
    }
  }, [adapterId, adapterAutoSelected, editingHarnessTargetId, editingTargetId, preferredDirectSetupAdapterId, runnableAdapters, setupIntent]);

  useEffect(() => {
    if (!modelBenchmarkPacks.length || modelBenchmarkPacks.some(pack => pack.id === comparisonPackId)) {
      return;
    }
    setComparisonPackId(recommendedComparisonPackId(packs));
  }, [comparisonPackId, modelBenchmarkPacks, packs]);

  useEffect(() => {
    if (!modelBenchmarkPacks.length || modelBenchmarkPacks.some(pack => pack.id === autoBenchmarkPackId)) {
      return;
    }
    setAutoBenchmarkPackId(recommendedComparisonPackId(packs));
  }, [autoBenchmarkPackId, modelBenchmarkPacks, packs]);

  useEffect(() => {
    if (
      autoLocalRuntimeDetectAttemptedRef.current
      || setupIntent
      || repairIntent
      || editingTargetId
      || editingHarnessTargetId
      || detectingLocal
      || localRuntimes.length
      || localComparisonTargetIds.length
      || localRuntimeDoctorCheck.check.status !== 'ok'
    ) {
      return;
    }
    autoLocalRuntimeDetectAttemptedRef.current = true;
    setAutoBenchmarkAfterAdd(true);
    setMessage('Ready local runtime found by Doctor. Detecting models so it can be added to the next benchmark.');
    void detectLocal().catch(error => setMessage(String(error)));
  }, [detectingLocal, editingHarnessTargetId, editingTargetId, localComparisonTargetIds.length, localRuntimes.length, localRuntimeDoctorCheck.check.status, repairIntent, setupIntent, setMessage]);

  useEffect(() => {
    if (!repairIntent) {
      return;
    }
    if (!targets.length && repairIntent.targetIds.length) {
      return;
    }
    const targetById = new Map(targets.map(target => [target.id, target]));
    const existingTargets = repairIntent.targetIds
      .map(id => targetById.get(id))
      .filter((target): target is Target => Boolean(target));
    const missingTargetIds = repairIntent.targetIds.filter(id => !targetById.has(id));
    const editableTargets = existingTargets.filter(targetRepairTargetCanEdit);
    onRepairIntentConsumed();
    if (editableTargets.length) {
      const target = editableTargets[0];
      void (async () => {
        if (target.kind === 'benchmark_harness') {
          await loadHarnessForEdit(target);
        } else {
          await loadTargetForEdit(target);
        }
        const skipped = repairIntent.targetIds.length > 1
          ? repairIntent.targetIds.filter(id => id !== target.id)
          : [];
        const skippedNote = skipped.length ? ` Other affected target(s): ${previewList(skipped)}.` : '';
        const firstNote = editableTargets.length > 1 ? ` Loaded ${target.name}, the first affected target, for editing.` : ` Loaded ${target.name} for editing.`;
        setMessage(`${errorCategoryRepairHint(repairIntent.code)}${firstNote}${skippedNote}`);
      })();
      return;
    }
    const unavailableTargetIds = existingTargets.map(target => target.id);
    const unavailableNote = unavailableTargetIds.length ? ` Uneditable target(s): ${previewList(unavailableTargetIds)}.` : '';
    const missingNote = missingTargetIds.length ? ` Missing target(s): ${previewList(missingTargetIds)}.` : '';
    setMessage(`${errorCategoryRepairHint(repairIntent.code)}${unavailableNote}${missingNote}`);
  }, [repairIntent, targets, onRepairIntentConsumed, setMessage]);

  useEffect(() => {
    if (!setupIntent) {
      return;
    }
    const setupBenchmarkPackId = resolveModelBenchmarkPackId(setupIntent.benchmarkPackId, modelBenchmarkPacks, packs);
    if (setupIntent.code === 'local_runtime_detect') {
      onSetupIntentConsumed();
      autoLocalRuntimeDetectAttemptedRef.current = true;
      clearTargetForm({ preserveAutoBenchmarkScope: true });
      clearHarnessForm();
      setAutoBenchmarkAfterAdd(true);
      setAutoBenchmarkPackId(setupBenchmarkPackId);
      setAutoBenchmarkTargetIds(uniqueIdsInOrder(setupIntent.targetIds ?? []));
      setComparisonPackId(setupBenchmarkPackId);
      setMessage(`Detecting local runtimes. Automatic benchmark after add will use ${benchmarkPackLabel(setupBenchmarkPackId, modelBenchmarkPacks)}.`);
      void detectLocal().catch(error => setMessage(String(error)));
      return;
    }
    if (!runnableAdapters.length) {
      return;
    }
    if (!setupIntent.adapterId) {
      onSetupIntentConsumed();
      setMessage('Choose a target setup path');
      return;
    }
    const adapter = runnableAdapters.find(item => item.id === setupIntent.adapterId);
    onSetupIntentConsumed();
    if (!adapter) {
      setMessage(`Cloud adapter ${setupIntent.adapterId} is not available in Targets`);
      return;
    }
    clearTargetForm({ preserveAutoBenchmarkScope: true });
    clearHarnessForm();
    setAutoBenchmarkAfterAdd(true);
    setAutoBenchmarkPackId(setupBenchmarkPackId);
    setAutoBenchmarkTargetIds(uniqueIdsInOrder(setupIntent.targetIds ?? []));
    setComparisonPackId(setupBenchmarkPackId);
    const defaultPreset = selectAdapter(adapter.id, { prefillFirstPreset: true });
    const presetHint = defaultPreset
      ? `; ${defaultPreset.label} is prefilled with pricing, so you can add the target or search for another model`
      : '; search the catalog or enter a model manually';
    const defaultActionHint = setupIntent.code === 'missing_key' ? 'Paste the API key' : 'Review the provider setup';
    const packHint = ` Automatic benchmark after add will use ${benchmarkPackLabel(setupBenchmarkPackId, modelBenchmarkPacks)}.`;
    const showSetupMessage = (actionHint: string) => setMessage(`${actionHint} for ${adapter.name}${presetHint}.${packHint}`);
    if (setupIntent.code === 'missing_key' && adapterNeedsApiKey(adapter, adapter.defaultBaseUrl ?? '')) {
      providerApiKeyStatus(providerKeychainId(adapter, adapter.defaultBaseUrl ?? ''))
        .then(status => {
          showSetupMessage(status.available ? 'API key is already available' : defaultActionHint);
        })
        .catch(() => showSetupMessage(defaultActionHint));
      return;
    }
    showSetupMessage(defaultActionHint);
  }, [setupIntent, runnableAdapters, modelBenchmarkPacks, packs, onSetupIntentConsumed, setMessage]);

  useEffect(() => {
    if (!selectedAdapter || !needsApiKey) {
      setProviderKeyAvailable(false);
      setProviderKeyDetail('');
      setProviderKeyStatusBusy(false);
      return;
    }
    let cancelled = false;
    setProviderKeyStatusBusy(true);
    providerApiKeyStatus(selectedProviderKeychainId)
      .then(status => {
        if (!cancelled) {
          setProviderKeyAvailable(status.available);
          setProviderKeyDetail(status.detail ?? '');
        }
      })
      .catch(error => {
        if (!cancelled) {
          setProviderKeyAvailable(false);
          setProviderKeyDetail('');
          setMessage(String(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setProviderKeyStatusBusy(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedAdapter?.id, selectedProviderKeychainId, needsApiKey]);

  function selectAdapter(nextId: string, options: { prefillFirstPreset?: boolean; autoSelected?: boolean } = {}) {
    const next = runnableAdapters.find(adapter => adapter.id === nextId);
    setAdapterId(nextId);
    setAdapterAutoSelected(Boolean(options.autoSelected));
    setEditingTargetPreserveApiKeyRef(false);
    setEditingTargetPreserveApiKeyEnvRef(false);
    setBaseUrl(next?.defaultBaseUrl ?? '');
    setApiKey('');
    setApiKeyEnv('');
    setCloudModels(adapterPresetCloudModels(next));
    setSelectedCloudModel(null);
    setTargetAdvancedOpen(false);
    if (next && options.prefillFirstPreset) {
      return applyFirstModelPreset(next);
    }
    setModelPresetId('custom');
    setModel('');
    setInputPrice('');
    setOutputPrice('');
    setCacheReadPrice('');
    setCacheWritePrice('');
    setCloudModelQuery('');
    return null;
  }

  function handleModelChange(nextModel: string) {
    setModel(nextModel);
    setSelectedCloudModel(null);
    const matchedPreset = matchingPricedModelPreset(selectedAdapter, nextModel);
    if (matchedPreset) {
      setModelPresetId(matchedPreset.id);
      setCloudModelQuery(matchedPreset.model);
      setInputPrice(matchedPreset.inputPrice != null ? String(matchedPreset.inputPrice) : '');
      setOutputPrice(matchedPreset.outputPrice != null ? String(matchedPreset.outputPrice) : '');
      setCacheReadPrice(matchedPreset.cacheReadPrice != null ? String(matchedPreset.cacheReadPrice) : '');
      setCacheWritePrice(matchedPreset.cacheWritePrice != null ? String(matchedPreset.cacheWritePrice) : '');
      return;
    }
    setModelPresetId('custom');
    setInputPrice('');
    setOutputPrice('');
    setCacheReadPrice('');
    setCacheWritePrice('');
  }

  function applyModelPreset(nextId: string) {
    setModelPresetId(nextId);
    setSelectedCloudModel(null);
    if (nextId === 'custom') {
      setInputPrice('');
      setOutputPrice('');
      setCacheReadPrice('');
      setCacheWritePrice('');
      return;
    }
    const preset = modelPresets.find(item => item.id === nextId);
    if (!preset) {
      return;
    }
    applyModelPresetValues(preset);
  }

  function applyModelPresetValues(preset: ModelPreset) {
    setModel(preset.model);
    setInputPrice(preset.inputPrice != null ? String(preset.inputPrice) : '');
    setOutputPrice(preset.outputPrice != null ? String(preset.outputPrice) : '');
    setCacheReadPrice(preset.cacheReadPrice != null ? String(preset.cacheReadPrice) : '');
    setCacheWritePrice(preset.cacheWritePrice != null ? String(preset.cacheWritePrice) : '');
  }

  function applyFirstModelPreset(adapter: Adapter) {
    const preset = adapterModelPresets(adapter)[0];
    if (!preset) {
      return null;
    }
    setModelPresetId(preset.id);
    setSelectedCloudModel(null);
    setCloudModelQuery('');
    applyModelPresetValues(preset);
    return preset;
  }

  function applyHarnessPreset(nextId: string) {
    setHarnessPresetId(nextId);
    setHarnessToolResult(null);
    const preset = harnessPresets.find(item => item.id === nextId);
    if (!preset) {
      return;
    }
    if (!editingHarnessTargetId) {
      setHarnessTargetId(preset.targetId);
    }
    setHarnessTargetName(preset.targetName);
    setHarnessCommand(preset.command);
    setHarnessModel(preset.defaultModel ?? '');
    setHarnessBaseUrl('');
    setHarnessTimeoutSeconds(preset.timeoutSeconds);
    setHarnessEnvPassthrough((preset.envPassthrough ?? []).join(', '));
    setHarnessSetupHint(preset.setupHint);
  }

  async function runSelectedHarnessTool(action: 'install' | 'check') {
    if (!selectedHarnessPreset) {
      setMessage('Choose a built-in harness preset first');
      return;
    }
    setHarnessToolBusy(action);
    try {
      const result = await runHarnessToolAction(selectedHarnessPreset.id, action);
      setHarnessToolResult(result);
      setMessage(`${selectedHarnessPreset.label} ${action} ${result.status}`);
    } catch (error) {
      setHarnessToolResult({
        presetId: selectedHarnessPreset.id,
        action,
        status: 'error',
        installCommand: selectedHarnessPreset.installCommand,
        checkCommand: selectedHarnessPreset.checkCommand,
        log: String(error),
      });
      setMessage(`${selectedHarnessPreset.label} ${action} failed`);
    } finally {
      setHarnessToolBusy('');
    }
  }

  async function refreshSelectedProviderKeyStatus(adapter = selectedAdapter) {
    if (!adapter || !adapterNeedsApiKey(adapter, baseUrl)) {
      setProviderKeyAvailable(false);
      setProviderKeyDetail('');
      return;
    }
    setProviderKeyStatusBusy(true);
    try {
      const status = await providerApiKeyStatus(providerKeychainId(adapter, baseUrl));
      setProviderKeyAvailable(status.available);
      setProviderKeyDetail(status.detail ?? '');
    } finally {
      setProviderKeyStatusBusy(false);
    }
  }

  async function ensureModelTargetHasRequiredKey(adapter: Adapter, keychainId: string, shouldPreserveKeyReference: boolean, shouldPreserveEnvReference: boolean) {
    if (!adapterNeedsApiKey(adapter, baseUrl)) {
      return true;
    }
    if (apiKey.trim() || apiKeyEnv.trim() || shouldPreserveKeyReference || shouldPreserveEnvReference) {
      return true;
    }
    setProviderKeyStatusBusy(true);
    try {
      const status = await providerApiKeyStatus(keychainId);
      setProviderKeyAvailable(status.available);
      setProviderKeyDetail(status.detail ?? '');
      if (status.available) {
        return true;
      }
      const envHint = status.envVar ? ` or set ${status.envVar}` : ' or configure an API key environment variable';
      setMessage(`Paste an API key for ${adapter.name}${envHint} before saving this cloud target.`);
      apiKeyInputRef.current?.focus();
      return false;
    } finally {
      setProviderKeyStatusBusy(false);
    }
  }

  async function searchModels() {
    if (!selectedAdapter) {
      setMessage('Select an adapter first');
      return;
    }
    setCloudModelBusy(true);
    try {
      let keyNote = '';
      if (apiKey.trim()) {
        await saveProviderApiKey(providerKeychainId(selectedAdapter, baseUrl), apiKey);
        setApiKey('');
        await refreshSelectedProviderKeyStatus(selectedAdapter);
        keyNote = 'Saved API key and ';
      }
      const nextModels = await searchCloudModels(
        selectedAdapter.id,
        cloudModelQuery,
        25,
        baseUrl,
        providerKeychainId(selectedAdapter, baseUrl),
        apiKeyEnv.trim() || undefined,
      );
      setCloudModels(nextModels);
      setMessage(`${keyNote}${nextModels.length} model(s) found for ${selectedAdapter.name}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setCloudModelBusy(false);
    }
  }

  function useCloudModel(nextModel: CloudModel) {
    setSelectedCloudModel(nextModel);
    setModelPresetId('custom');
    setModel(nextModel.model);
    setInputPrice(nextModel.inputPriceUsdPerMillionTokens != null ? String(nextModel.inputPriceUsdPerMillionTokens) : '');
    setOutputPrice(nextModel.outputPriceUsdPerMillionTokens != null ? String(nextModel.outputPriceUsdPerMillionTokens) : '');
    setCacheReadPrice(nextModel.cacheReadPriceUsdPerMillionTokens != null ? String(nextModel.cacheReadPriceUsdPerMillionTokens) : '');
    setCacheWritePrice(nextModel.cacheWritePriceUsdPerMillionTokens != null ? String(nextModel.cacheWritePriceUsdPerMillionTokens) : '');
    setMessage(`Selected ${nextModel.model}`);
  }

  function cloudCatalogTargetName(nextModel: CloudModel) {
    return targetName.trim() || `${selectedAdapter?.name ?? nextModel.provider} ${nextModel.model}`;
  }

  function cloudCatalogTargetId(nextModel: CloudModel) {
    return slugify(`${selectedAdapter?.id ?? nextModel.provider}-${nextModel.model}`);
  }

  function cloudCatalogPricingTarget(nextModel: CloudModel) {
    return {
      inputPriceUsdPerMillionTokens: nextModel.inputPriceUsdPerMillionTokens ?? undefined,
      outputPriceUsdPerMillionTokens: nextModel.outputPriceUsdPerMillionTokens ?? undefined,
      cacheReadPriceUsdPerMillionTokens: nextModel.cacheReadPriceUsdPerMillionTokens ?? undefined,
      cacheWritePriceUsdPerMillionTokens: nextModel.cacheWritePriceUsdPerMillionTokens ?? undefined,
    };
  }

  function cloudCatalogPlannedTarget(nextModel: CloudModel): Target | null {
    if (!selectedAdapter) {
      return null;
    }
    return plannedModelTarget(
      cloudCatalogTargetId(nextModel),
      cloudCatalogTargetName(nextModel),
      selectedAdapter.id,
      baseUrl,
      cloudCatalogPricingTarget(nextModel),
    );
  }

  function cloudCatalogBenchmarkIntent(nextModel: CloudModel) {
    const plannedTarget = cloudCatalogPlannedTarget(nextModel);
    if (!plannedTarget) {
      return null;
    }
    const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
    const targetUniverse = targetListWithOverride(plannedTarget, targets);
    return automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
  }

  function cloudCatalogNeedsKeyBeforeAdd() {
    return Boolean(selectedAdapter
      && adapterNeedsApiKey(selectedAdapter, baseUrl)
      && !apiKey.trim()
      && !apiKeyEnv.trim()
      && !providerKeyAvailable);
  }

  function cloudCatalogActionLabel(nextModel: CloudModel) {
    if (cloudCatalogNeedsKeyBeforeAdd()) {
      return 'Add key';
    }
    if (!autoBenchmarkAfterAdd) {
      return 'Add target';
    }
    const intent = cloudCatalogBenchmarkIntent(nextModel);
    const plannedTarget = cloudCatalogPlannedTarget(nextModel);
    const targetUniverse = plannedTarget ? targetListWithOverride(plannedTarget, targets) : targets;
    if (intent && cappedIntentHasUnpricedCloudTarget(intent, targetUniverse)) {
      return 'Add + price';
    }
    return intent && intent.targetIds.length > 1 ? 'Add + compare' : 'Add + run';
  }

  async function addCloudModelFromCatalog(nextModel: CloudModel) {
    if (!selectedAdapter) {
      setMessage('Select an adapter first');
      return;
    }
    if (editingTargetId) {
      setMessage('Finish or cancel the current target edit before adding a catalog model.');
      return;
    }
    const busyId = `${nextModel.source}:${nextModel.model}`;
    setCatalogAddBusyModel(busyId);
    useCloudModel(nextModel);
    try {
      const keychainId = providerKeychainId(selectedAdapter, baseUrl);
      if (apiKeyEnv.trim() && !isValidEnvName(apiKeyEnv.trim())) {
        setMessage('API key env var must be a valid environment variable name, for example OPENAI_API_KEY.');
        return;
      }
      if (!(await ensureModelTargetHasRequiredKey(selectedAdapter, keychainId, false, false))) {
        return;
      }
      if (apiKey.trim()) {
        await saveProviderApiKey(keychainId, apiKey);
        setApiKey('');
        await refreshSelectedProviderKeyStatus(selectedAdapter);
      }
      const parsedTemperature = parseOptionalNumberInRange(temperature, 'Temperature', 0, 2);
      const parsedTopP = parseOptionalNumberInRange(topP, 'Top P', 0, 1);
      const parsedMaxTokens = parseOptionalPositiveInteger(maxTokens, 'Max tokens');
      const parsedSeed = parseOptionalInteger(seed, 'Seed');
      const parsedTimeout = parseOptionalPositiveInteger(timeoutSeconds, 'Timeout');
      const parsedRetryCount = parseOptionalIntegerInRange(retryCount, 'Retries', 0, 5);
      const validationError = parsedTemperature.error
        || parsedTopP.error
        || parsedMaxTokens.error
        || parsedSeed.error
        || parsedTimeout.error
        || parsedRetryCount.error;
      if (validationError) {
        setMessage(validationError);
        return;
      }
      const hasInputPrice = nextModel.inputPriceUsdPerMillionTokens != null;
      const hasOutputPrice = nextModel.outputPriceUsdPerMillionTokens != null;
      if (hasInputPrice !== hasOutputPrice) {
        setMessage(`Catalog pricing for ${nextModel.model} is incomplete. Use the row first, review pricing, then add the target.`);
        return;
      }
      if ((nextModel.cacheReadPriceUsdPerMillionTokens != null || nextModel.cacheWritePriceUsdPerMillionTokens != null)
        && (!hasInputPrice || !hasOutputPrice)) {
        setMessage(`Catalog cache pricing for ${nextModel.model} needs input and output prices. Use the row first, review pricing, then add the target.`);
        return;
      }
      const config: Record<string, unknown> = {
        model: nextModel.model,
        api_key_keychain: keychainId,
        pricing_preset: nextModel.name || nextModel.model,
        pricing_source: nextModel.sourceUrl || nextModel.source,
        pricing_provider: nextModel.provider,
      };
      if (apiKeyEnv.trim()) {
        config.api_key_env = apiKeyEnv.trim();
      }
      if (baseUrl.trim()) {
        config.base_url = baseUrl.trim();
      }
      if (nextModel.inputPriceUsdPerMillionTokens != null) {
        config.input_price_usd_per_million_tokens = nextModel.inputPriceUsdPerMillionTokens;
      }
      if (nextModel.outputPriceUsdPerMillionTokens != null) {
        config.output_price_usd_per_million_tokens = nextModel.outputPriceUsdPerMillionTokens;
      }
      if (nextModel.cacheReadPriceUsdPerMillionTokens != null) {
        config.cache_read_price_usd_per_million_tokens = nextModel.cacheReadPriceUsdPerMillionTokens;
      }
      if (nextModel.cacheWritePriceUsdPerMillionTokens != null) {
        config.cache_write_price_usd_per_million_tokens = nextModel.cacheWritePriceUsdPerMillionTokens;
      }
      if (nextModel.contextLength != null) {
        config.context_length = nextModel.contextLength;
      }
      if (nextModel.detail) {
        config.pricing_note = nextModel.detail;
      }
      if (parsedTemperature.value != null) {
        config.temperature = parsedTemperature.value;
      }
      if (parsedTopP.value != null) {
        config.top_p = parsedTopP.value;
      }
      if (parsedMaxTokens.value != null) {
        config.max_tokens = parsedMaxTokens.value;
      }
      if (parsedSeed.value != null) {
        config.seed = parsedSeed.value;
      }
      if (parsedTimeout.value != null) {
        config.timeout_seconds = parsedTimeout.value;
      }
      if (parsedRetryCount.value != null) {
        config.retry_count = parsedRetryCount.value;
      }
      const name = cloudCatalogTargetName(nextModel);
      const id = cloudCatalogTargetId(nextModel);
      const targetRequest = { id, name, kind: 'direct_model', adapterId: selectedAdapter.id, config };
      const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
      const plannedTarget = plannedModelTarget(id, name, selectedAdapter.id, baseUrl, cloudCatalogPricingTarget(nextModel));
      const targetUniverse = targetListWithOverride(plannedTarget, targets);
      const benchmarkIntent = automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
      const autoBenchmarkPricingTargetIds = autoBenchmarkAfterAdd
        ? unpricedCloudTargetIdsForIntent(benchmarkIntent, targetUniverse)
        : [];
      const autoBenchmarkNeedsPricing = autoBenchmarkPricingTargetIds.length > 0;
      setMessage(`Adding ${nextModel.model} and validating target`);
      const handoff = await createTargetWithBenchmarkHandoff(
        targetRequest,
        autoBenchmarkAfterAdd && !autoBenchmarkNeedsPricing
          ? {
              benchmarkPackId: packId,
              benchmarkTargetIds: benchmarkIntent.targetIds,
              repetitions: benchmarkIntent.repetitions,
              warmupRuns: benchmarkIntent.warmupRuns,
              concurrency: benchmarkIntent.concurrency,
              maxCostUsd: benchmarkIntent.maxCostUsd,
            }
          : {},
      );
      setValidations(current => {
        const next = { ...current };
        if (handoff.validation) {
          next[id] = handoff.validation;
        } else {
          delete next[id];
        }
        return next;
      });
      await finishModelTargetHandoff(
        handoff.target,
        handoff.validation ?? null,
        name,
        false,
        handoff.runJob ?? null,
        handoff.benchmarkError ?? null,
        false,
        {
          pendingAutoBenchmarkPackId: autoBenchmarkNeedsPricing ? packId : '',
          pendingAutoBenchmarkPricingTargetIds: autoBenchmarkPricingTargetIds,
        },
      );
    } finally {
      setCatalogAddBusyModel('');
    }
  }

  function clearTargetForm(options: { preserveAutoBenchmarkScope?: boolean } = {}) {
    const adapter = selectedAdapter ?? runnableAdapters[0];
    setEditingTargetId('');
    setTargetName('');
    setAdapterId(adapter?.id ?? '');
    setAdapterAutoSelected(false);
    setBaseUrl(adapter?.defaultBaseUrl ?? '');
    setApiKey('');
    setApiKeyEnv('');
    setTemperature('0');
    setTopP('1');
    setMaxTokens('512');
    setSeed('');
    setTimeoutSeconds('120');
    setRetryCount('1');
    setEditingTargetHadValidationError(false);
    setEditingTargetPreserveApiKeyRef(false);
    setEditingTargetPreserveApiKeyEnvRef(false);
    setEditingTargetPendingAutoBenchmarkPackId('');
    setCloudModels(adapterPresetCloudModels(adapter));
    setSelectedCloudModel(null);
    setTargetAdvancedOpen(false);
    if (!options.preserveAutoBenchmarkScope) {
      setAutoBenchmarkTargetIds([]);
    }
    if (adapter) {
      const defaultPreset = applyFirstModelPreset(adapter);
      if (defaultPreset) {
        return;
      }
    }
    setModel('');
    setInputPrice('');
    setOutputPrice('');
    setCacheReadPrice('');
    setCacheWritePrice('');
    setModelPresetId('custom');
    setCloudModelQuery('');
  }

  function clearHarnessForm() {
    setEditingHarnessTargetId('');
    setHarnessPresetId('custom');
    setHarnessTargetId('benchforge-worker');
    setHarnessTargetName('BenchForge Worker');
    setHarnessCommand('');
    setHarnessModel('');
    setHarnessBaseUrl('');
    setHarnessTimeoutSeconds('3600');
    setHarnessEnvPassthrough('');
    setHarnessSetupHint('');
  }

  async function loadTargetForEdit(target: Target, options: { pendingAutoBenchmarkPackId?: string } = {}) {
    if (target.kind !== 'direct_model') {
      setMessage('Only direct model targets can be edited in this form');
      return;
    }
    setLoadingTargetId(target.id);
    try {
      const exported = await exportTargetRedacted(target.id);
      const adapter = runnableAdapters.find(item => item.id === exported.adapter_id || item.id === target.adapterId);
      if (!adapter) {
        setMessage(`Target ${target.name} uses adapter ${exported.adapter_id}, which is not available in this editor`);
        return;
      }
      const config = exported.config ?? {};
      if (!options.pendingAutoBenchmarkPackId) {
        setAutoBenchmarkTargetIds([]);
      }
      const modelValue = formValueFromUnknown(config.model);
      const inputPriceValue = formValueFromUnknown(config.input_price_usd_per_million_tokens);
      const outputPriceValue = formValueFromUnknown(config.output_price_usd_per_million_tokens);
      const cacheReadPriceValue = formValueFromUnknown(config.cache_read_price_usd_per_million_tokens, formValueFromUnknown(config.cached_input_price_usd_per_million_tokens));
      const cacheWritePriceValue = formValueFromUnknown(config.cache_write_price_usd_per_million_tokens, formValueFromUnknown(config.cache_creation_price_usd_per_million_tokens));
      const matchingPreset = matchingPricedModelPreset(adapter, modelValue);
      const matchedPresetFillsMissingPricing = Boolean(matchingPreset && (
        (!inputPriceValue && matchingPreset.inputPrice != null)
        || (!outputPriceValue && matchingPreset.outputPrice != null)
        || (!cacheReadPriceValue && matchingPreset.cacheReadPrice != null)
        || (!cacheWritePriceValue && matchingPreset.cacheWritePrice != null)
      ));
      setEditingTargetId(exported.id);
      setEditingTargetHadValidationError(target.validationStatus === 'error');
      setEditingTargetPreserveApiKeyRef(config.api_key_keychain === '[REDACTED]');
      setEditingTargetPreserveApiKeyEnvRef(config.api_key_env === '[REDACTED]');
      setEditingTargetPendingAutoBenchmarkPackId(options.pendingAutoBenchmarkPackId ?? '');
      setAdapterId(adapter.id);
      setAdapterAutoSelected(false);
      setTargetName(exported.name);
      setModel(modelValue);
      setBaseUrl(formValueFromUnknown(config.base_url, adapter.defaultBaseUrl ?? ''));
      setApiKey('');
      setApiKeyEnv(config.api_key_env === '[REDACTED]' ? '' : formValueFromUnknown(config.api_key_env));
      setTemperature(formValueFromUnknown(config.temperature, '0'));
      setTopP(formValueFromUnknown(config.top_p, '1'));
      setMaxTokens(formValueFromUnknown(config.max_tokens, '512'));
      setSeed(formValueFromUnknown(config.seed));
      setTimeoutSeconds(formValueFromUnknown(config.timeout_seconds, '120'));
      setRetryCount(formValueFromUnknown(config.retry_count, '1'));
      setInputPrice(inputPriceValue || (matchingPreset?.inputPrice != null ? String(matchingPreset.inputPrice) : ''));
      setOutputPrice(outputPriceValue || (matchingPreset?.outputPrice != null ? String(matchingPreset.outputPrice) : ''));
      setCacheReadPrice(cacheReadPriceValue || (matchingPreset?.cacheReadPrice != null ? String(matchingPreset.cacheReadPrice) : ''));
      setCacheWritePrice(cacheWritePriceValue || (matchingPreset?.cacheWritePrice != null ? String(matchingPreset.cacheWritePrice) : ''));
      setModelPresetId(matchedPresetFillsMissingPricing && matchingPreset ? matchingPreset.id : 'custom');
      setCloudModelQuery(matchedPresetFillsMissingPricing && matchingPreset ? matchingPreset.model : '');
      setCloudModels([]);
      setSelectedCloudModel(null);
      setTargetAdvancedOpen(true);
      const pricingNote = matchedPresetFillsMissingPricing && matchingPreset ? `; prefilled missing pricing from ${matchingPreset.label}` : '';
      const pendingNote = options.pendingAutoBenchmarkPackId ? '; update this target to continue the pending automatic benchmark' : '';
      setMessage(`Loaded ${exported.name} for editing${pricingNote}${pendingNote}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setLoadingTargetId('');
    }
  }

  async function loadHarnessForEdit(target: Target) {
    if (target.kind !== 'benchmark_harness') {
      setMessage('Only worker harness targets can be edited in this form');
      return;
    }
    setLoadingTargetId(target.id);
    try {
      const exported = await exportTargetRedacted(target.id);
      const config = exported.config ?? {};
      const harness = isRecord(config.harness) ? config.harness : {};
      const commandValue = formValueFromUnknown(harness.command);
      setEditingHarnessTargetId(exported.id);
      setHarnessTargetId(exported.id);
      setHarnessTargetName(exported.name);
      setHarnessPresetId(harnessPresetIdFromConfig(formValueFromUnknown(harness.preset), commandValue));
      setHarnessCommand(commandValue);
      setHarnessModel(formValueFromUnknown(harness.model));
      setHarnessBaseUrl(formValueFromUnknown(harness.base_url));
      setHarnessTimeoutSeconds(formValueFromUnknown(harness.timeout_seconds, formValueFromUnknown(config.timeout_seconds, '3600')));
      setHarnessEnvPassthrough(formValueFromEnvPassthrough(harness.env_passthrough));
      setHarnessSetupHint(formValueFromUnknown(harness.setup_hint));
      setMessage(`Loaded ${exported.name} for editing`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setLoadingTargetId('');
    }
  }

  async function addMock() {
    await createTarget({ id: 'mock-agent', name: 'Mock Agent', kind: 'mock', adapterId: 'mock', config: { mode: 'deterministic-fixture-fix' } });
    await onRefresh();
    setMessage('Mock target ready');
  }

  async function addWorkerHarness() {
    const target = await createTarget({ id: 'benchforge-worker', name: 'BenchForge Worker', kind: 'benchmark_harness', adapterId: 'benchforge-worker', config: { command: 'benchforge-worker' } });
    const validation = await validateSavedTarget(target.id);
    await onRefresh();
    setMessage(validation ? `BenchForge Worker harness saved; validation ${validation.status}: ${validation.detail}` : 'BenchForge Worker harness saved, but validation could not run');
  }

  async function validateSavedTarget(targetId: string): Promise<TargetValidation | null> {
    setValidating(targetId);
    try {
      const result = await validateTarget(targetId);
      setValidations(current => ({ ...current, [targetId]: result }));
      return result;
    } catch (error) {
      setMessage(`Target saved, but validation failed to run: ${String(error)}`);
      return null;
    } finally {
      setValidating('');
    }
  }

  async function addOrUpdateWorkerHarness() {
    const rawTargetId = harnessTargetId.trim();
    if (!editingHarnessTargetId && !rawTargetId) {
      setMessage('Target ID is required');
      return;
    }
    const targetId = editingHarnessTargetId || rawTargetId.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 64);
    if (!targetId) {
      setMessage('Target ID must include at least one letter or number');
      return;
    }
    const parsedTimeout = parseOptionalPositiveInteger(harnessTimeoutSeconds, 'Harness timeout');
    if (parsedTimeout.error) {
      setMessage(parsedTimeout.error);
      return;
    }
    const command = harnessCommand.trim();
    const name = harnessTargetName.trim() || 'BenchForge Worker';
    const config: Record<string, unknown> = { command: 'benchforge-worker' };
    if (command) {
      const harnessConfig: Record<string, unknown> = {
        command,
        timeout_seconds: parsedTimeout.value ?? 3600,
      };
      if (harnessPresetId !== 'custom') {
        harnessConfig.preset = harnessPresetId;
      }
      if (harnessModel.trim()) {
        harnessConfig.model = harnessModel.trim();
      }
      if (harnessBaseUrl.trim()) {
        harnessConfig.base_url = harnessBaseUrl.trim();
      }
      const envPassthrough = envPassthroughFromText(harnessEnvPassthrough);
      if (envPassthrough.length) {
        harnessConfig.env_passthrough = envPassthrough;
      }
      const setupHint = harnessSetupHint.trim() || selectedHarnessPreset?.setupHint;
      if (setupHint) {
        harnessConfig.setup_hint = setupHint;
      }
      if (selectedHarnessPreset?.outputHint) {
        harnessConfig.output_hint = selectedHarnessPreset.outputHint;
      }
      config.harness = harnessConfig;
    } else if (parsedTimeout.value != null) {
      config.timeout_seconds = parsedTimeout.value;
    }
    const wasEditing = Boolean(editingHarnessTargetId);
    const target = await createTarget({
      id: targetId,
      name,
      kind: 'benchmark_harness',
      adapterId: 'benchforge-worker',
      config,
    });
    setValidations(current => {
      const next = { ...current };
      delete next[targetId];
      return next;
    });
    setEditingHarnessTargetId('');
    const validation = await validateSavedTarget(target.id);
    await onRefresh();
    if (!validation) {
      setMessage(`Harness target ${name} saved, but validation could not run`);
      return;
    }
    if (validation.status === 'error') {
      setMessage(`Harness target ${name} saved, but validation failed: ${validation.detail}`);
      return;
    }
    if (!wasEditing) {
      openRunBuilder(runBuilderIntentForTarget(target, selectedHarnessPreset?.benchmarkPackId ?? recommendedHarnessPackForCommand(command)));
    }
    const validationNote = validation.status === 'ok' ? 'validated' : `saved with warning: ${validation.detail}`;
    setMessage(wasEditing ? `Updated harness target ${name}; ${validationNote}` : `Harness target ${name} ${validationNote} and ready in Runs`);
  }

  async function detectLocal() {
    setDetectingLocal(true);
    try {
      const detected = await detectLocalRuntimes();
      setLocalRuntimes(detected);
      setLocalSelections(Object.fromEntries(detected.map(runtime => [runtime.id, runtime.recommendedModel ?? runtime.models[0] ?? ''])));
      const ready = detected.filter(runtime => runtime.status === 'ok').length;
      setMessage(`${ready}/${detected.length} local runtimes detected`);
    } finally {
      setDetectingLocal(false);
    }
  }

  async function runSelectedLocalRuntimeTool(runtime: LocalRuntime, action: 'install' | 'check' | 'pull', model?: string) {
    setRuntimeToolBusy(`${runtime.id}:${action}`);
    try {
      const result = await runLocalRuntimeToolAction(runtime.id, action, model);
      setRuntimeToolResult(result);
      if (action === 'pull' && result.status === 'ready') {
        const detected = await detectLocalRuntimes();
        setLocalRuntimes(detected);
        setLocalSelections(current => {
          const defaults = Object.fromEntries(detected.map(item => [item.id, item.recommendedModel ?? item.models[0] ?? '']));
          return { ...defaults, ...current, [runtime.id]: model ?? current[runtime.id] ?? '' };
        });
      }
      setMessage(`${runtime.name} ${action}${model ? ` ${model}` : ''} ${result.status}`);
    } catch (error) {
      setRuntimeToolResult({
        runtimeId: runtime.id,
        action,
        status: 'error',
        installCommand: runtime.installCommand ?? null,
        checkCommand: runtime.startCommand ?? runtime.baseUrl,
        log: String(error),
      });
      setMessage(`${runtime.name} ${action} failed`);
    } finally {
      setRuntimeToolBusy('');
    }
  }

  function localRuntimeTargetName(runtime: LocalRuntime, model: string) {
    return `${runtime.name} ${model.trim()}`;
  }

  function localRuntimeTargetId(runtime: LocalRuntime, model: string) {
    return slugify(`${runtime.adapterId}-${model.trim()}-${runtime.baseUrl}`);
  }

  function localRuntimePlannedTarget(runtime: LocalRuntime, model: string) {
    return plannedModelTarget(
      localRuntimeTargetId(runtime, model),
      localRuntimeTargetName(runtime, model),
      runtime.adapterId,
      runtime.baseUrl,
    );
  }

  function localRuntimeBenchmarkIntent(runtime: LocalRuntime, model: string) {
    if (!model.trim() || !localRuntimeCanAddTarget(runtime, model)) {
      return null;
    }
    const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
    const plannedTarget = localRuntimePlannedTarget(runtime, model);
    const targetUniverse = targetListWithOverride(plannedTarget, targets);
    return automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
  }

  function localRuntimeActionLabel(runtime: LocalRuntime, model: string) {
    if (!localRuntimeCanAddTarget(runtime, model)) {
      return 'Add target';
    }
    if (!autoBenchmarkAfterAdd) {
      return 'Add target';
    }
    const intent = localRuntimeBenchmarkIntent(runtime, model);
    if (intent && cappedIntentHasUnpricedCloudTarget(intent, targetListWithOverride(localRuntimePlannedTarget(runtime, model), targets))) {
      return 'Add + price';
    }
    return intent && intent.targetIds.length > 1 ? 'Add + compare' : 'Add + run';
  }

  async function addDetectedRuntime(runtime: LocalRuntime, model: string) {
    if (!model.trim()) {
      setMessage('Select a local model first');
      return;
    }
    const name = localRuntimeTargetName(runtime, model);
    const id = localRuntimeTargetId(runtime, model);
    setAddingLocalRuntimeId(runtime.id);
    setMessage(`Adding ${name} and validating target`);
    const targetRequest = {
      id,
      name,
      kind: 'direct_model',
      adapterId: runtime.adapterId,
      config: {
        model: model.trim(),
        base_url: runtime.baseUrl,
        source: 'local-runtime-detect',
        runtime: {
          id: runtime.id,
          name: runtime.name,
          adapter_id: runtime.adapterId,
          base_url: runtime.baseUrl,
          detected_status: runtime.status,
          detected_detail: runtime.detail,
          detected_at: runtime.detectedAt,
          probe_url: runtime.probeUrl ?? null,
          model_source: runtime.modelSource ?? null,
          model_count: runtime.models.length,
          models: runtime.models.slice(0, 50),
          recommended_model: runtime.recommendedModel ?? null,
          selected_model: model.trim(),
        },
        temperature: 0,
        top_p: 1,
        max_tokens: 512,
        timeout_seconds: 120,
        retry_count: 1,
        input_price_usd_per_million_tokens: 0,
        output_price_usd_per_million_tokens: 0,
      },
    };
    try {
      const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
      const plannedTarget = localRuntimePlannedTarget(runtime, model);
      const targetUniverse = targetListWithOverride(plannedTarget, targets);
      const benchmarkIntent = automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
      const autoBenchmarkPricingTargetIds = autoBenchmarkAfterAdd
        ? unpricedCloudTargetIdsForIntent(benchmarkIntent, targetUniverse)
        : [];
      const autoBenchmarkNeedsPricing = autoBenchmarkPricingTargetIds.length > 0;
      const handoff = await createTargetWithBenchmarkHandoff(
        targetRequest,
        autoBenchmarkAfterAdd && !autoBenchmarkNeedsPricing
          ? {
              benchmarkPackId: packId,
              benchmarkTargetIds: benchmarkIntent.targetIds,
              repetitions: benchmarkIntent.repetitions,
              warmupRuns: benchmarkIntent.warmupRuns,
              concurrency: benchmarkIntent.concurrency,
              maxCostUsd: benchmarkIntent.maxCostUsd,
            }
          : {},
      );
      setValidations(current => {
        const next = { ...current };
        if (handoff.validation) {
          next[id] = handoff.validation;
        } else {
          delete next[id];
        }
        return next;
      });
      await finishModelTargetHandoff(
        handoff.target,
        handoff.validation ?? null,
        name,
        false,
        handoff.runJob ?? null,
        handoff.benchmarkError ?? null,
        false,
        {
          pendingAutoBenchmarkPackId: autoBenchmarkNeedsPricing ? packId : '',
          pendingAutoBenchmarkPricingTargetIds: autoBenchmarkPricingTargetIds,
        },
      );
    } finally {
      setAddingLocalRuntimeId('');
    }
  }

  async function addModelTarget() {
    if (!selectedAdapter) {
      setMessage('Select an adapter first');
      return;
    }
    if (!model.trim()) {
      setMessage('Model is required');
      return;
    }
    const wasEditing = Boolean(editingTargetId);
    const wasRepairingValidationError = wasEditing && editingTargetHadValidationError;
    const pendingAutoBenchmarkPackId = wasEditing ? editingTargetPendingAutoBenchmarkPackId : '';
    const keychainId = providerKeychainId(selectedAdapter, baseUrl);
    const shouldPreserveKeyReference = wasEditing && !apiKey.trim() && editingTargetPreserveApiKeyRef;
    const shouldPreserveEnvReference = wasEditing && !apiKeyEnv.trim() && editingTargetPreserveApiKeyEnvRef;
    if (apiKeyEnv.trim() && !isValidEnvName(apiKeyEnv.trim())) {
      setMessage('API key env var must be a valid environment variable name, for example OPENAI_API_KEY.');
      return;
    }
    if (!(await ensureModelTargetHasRequiredKey(selectedAdapter, keychainId, shouldPreserveKeyReference, shouldPreserveEnvReference))) {
      return;
    }
    if (apiKey.trim()) {
      await saveProviderApiKey(keychainId, apiKey);
      setApiKey('');
      await refreshSelectedProviderKeyStatus(selectedAdapter);
    }
    const parsedInputPrice = parseOptionalNonNegativeNumber(inputPrice, 'Input price');
    const parsedOutputPrice = parseOptionalNonNegativeNumber(outputPrice, 'Output price');
    const parsedCacheReadPrice = parseOptionalNonNegativeNumber(cacheReadPrice, 'Cache read price');
    const parsedCacheWritePrice = parseOptionalNonNegativeNumber(cacheWritePrice, 'Cache write price');
    const parsedTemperature = parseOptionalNumberInRange(temperature, 'Temperature', 0, 2);
    const parsedTopP = parseOptionalNumberInRange(topP, 'Top P', 0, 1);
    const parsedMaxTokens = parseOptionalPositiveInteger(maxTokens, 'Max tokens');
    const parsedSeed = parseOptionalInteger(seed, 'Seed');
    const parsedTimeout = parseOptionalPositiveInteger(timeoutSeconds, 'Timeout');
    const parsedRetryCount = parseOptionalIntegerInRange(retryCount, 'Retries', 0, 5);
    const validationError = parsedInputPrice.error
      || parsedOutputPrice.error
      || parsedCacheReadPrice.error
      || parsedCacheWritePrice.error
      || parsedTemperature.error
      || parsedTopP.error
      || parsedMaxTokens.error
      || parsedSeed.error
      || parsedTimeout.error
      || parsedRetryCount.error;
    if (validationError) {
      setMessage(validationError);
      return;
    }
    if ((parsedInputPrice.value != null) !== (parsedOutputPrice.value != null)) {
      setMessage('Enter both input and output prices per 1M tokens, or leave both blank.');
      return;
    }
    if ((parsedCacheReadPrice.value != null || parsedCacheWritePrice.value != null)
      && (parsedInputPrice.value == null || parsedOutputPrice.value == null)) {
      setMessage('Cache pricing requires input and output prices per 1M tokens.');
      return;
    }
    const config: Record<string, unknown> = {
      model: model.trim(),
      api_key_keychain: shouldPreserveKeyReference ? '[REDACTED]' : keychainId,
    };
    if (apiKeyEnv.trim()) {
      config.api_key_env = apiKeyEnv.trim();
    } else if (shouldPreserveEnvReference) {
      config.api_key_env = '[REDACTED]';
    }
    if (baseUrl.trim()) {
      config.base_url = baseUrl.trim();
    }
    if (parsedInputPrice.value != null) {
      config.input_price_usd_per_million_tokens = parsedInputPrice.value;
    }
    if (parsedOutputPrice.value != null) {
      config.output_price_usd_per_million_tokens = parsedOutputPrice.value;
    }
    if (parsedCacheReadPrice.value != null) {
      config.cache_read_price_usd_per_million_tokens = parsedCacheReadPrice.value;
    }
    if (parsedCacheWritePrice.value != null) {
      config.cache_write_price_usd_per_million_tokens = parsedCacheWritePrice.value;
    }
    if (parsedTemperature.value != null) {
      config.temperature = parsedTemperature.value;
    }
    if (parsedTopP.value != null) {
      config.top_p = parsedTopP.value;
    }
    if (parsedMaxTokens.value != null) {
      config.max_tokens = parsedMaxTokens.value;
    }
    if (parsedSeed.value != null) {
      config.seed = parsedSeed.value;
    }
    if (parsedTimeout.value != null) {
      config.timeout_seconds = parsedTimeout.value;
    }
    if (parsedRetryCount.value != null) {
      config.retry_count = parsedRetryCount.value;
    }
    if (selectedCloudModel && selectedCloudModel.model === model.trim()) {
      config.pricing_preset = selectedCloudModel.name || selectedCloudModel.model;
      config.pricing_source = selectedCloudModel.sourceUrl || selectedCloudModel.source;
      config.pricing_provider = selectedCloudModel.provider;
      if (selectedCloudModel.contextLength != null) {
        config.context_length = selectedCloudModel.contextLength;
      }
      if (selectedCloudModel.detail) {
        config.pricing_note = selectedCloudModel.detail;
      }
    } else if (selectedModelPreset && selectedModelPreset.model === model.trim()) {
      config.pricing_preset = selectedModelPreset.label;
      if (selectedModelPreset.source) {
        config.pricing_source = selectedModelPreset.source;
      } else if (typeof selectedAdapter.metadata.pricing_source === 'string') {
        config.pricing_source = selectedAdapter.metadata.pricing_source;
      }
      if (typeof selectedAdapter.metadata.pricing_verified_at === 'string') {
        config.pricing_verified_at = selectedAdapter.metadata.pricing_verified_at;
      }
      if (selectedModelPreset.note) {
        config.pricing_note = selectedModelPreset.note;
      }
    }
    if ((parsedInputPrice.value != null || parsedOutputPrice.value != null || parsedCacheReadPrice.value != null || parsedCacheWritePrice.value != null) && typeof config.pricing_source !== 'string') {
      config.pricing_source = 'manual';
    }
    const name = targetName.trim() || `${selectedAdapter.name} ${model.trim()}`;
    const id = editingTargetId || slugify(`${selectedAdapter.id}-${model.trim()}`);
    const targetRequest = { id, name, kind: 'direct_model', adapterId: selectedAdapter.id, config };
    if (!wasEditing) {
      const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
      const plannedTarget = plannedModelTarget(id, name, selectedAdapter.id, baseUrl, {
        inputPriceUsdPerMillionTokens: parsedInputPrice.value,
        outputPriceUsdPerMillionTokens: parsedOutputPrice.value,
        cacheReadPriceUsdPerMillionTokens: parsedCacheReadPrice.value,
        cacheWritePriceUsdPerMillionTokens: parsedCacheWritePrice.value,
      });
      const targetUniverse = targetListWithOverride(plannedTarget, targets);
      const benchmarkIntent = automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
      const autoBenchmarkPricingTargetIds = autoBenchmarkAfterAdd
        ? unpricedCloudTargetIdsForIntent(benchmarkIntent, targetUniverse)
        : [];
      const autoBenchmarkNeedsPricing = autoBenchmarkPricingTargetIds.length > 0;
      const handoff = await createTargetWithBenchmarkHandoff(
        targetRequest,
        autoBenchmarkAfterAdd && !autoBenchmarkNeedsPricing
          ? {
              benchmarkPackId: packId,
              benchmarkTargetIds: benchmarkIntent.targetIds,
              repetitions: benchmarkIntent.repetitions,
              warmupRuns: benchmarkIntent.warmupRuns,
              concurrency: benchmarkIntent.concurrency,
              maxCostUsd: benchmarkIntent.maxCostUsd,
            }
          : {},
      );
      setValidations(current => {
        const next = { ...current };
        if (handoff.validation) {
          next[id] = handoff.validation;
        } else {
          delete next[id];
        }
        return next;
      });
      setEditingTargetId('');
      setEditingTargetHadValidationError(false);
      setEditingTargetPreserveApiKeyRef(false);
      setEditingTargetPreserveApiKeyEnvRef(false);
      await finishModelTargetHandoff(
        handoff.target,
        handoff.validation ?? null,
        name,
        false,
        handoff.runJob ?? null,
        handoff.benchmarkError ?? null,
        false,
        {
          pendingAutoBenchmarkPackId: autoBenchmarkNeedsPricing ? packId : '',
          pendingAutoBenchmarkPricingTargetIds: autoBenchmarkPricingTargetIds,
        },
      );
      return;
    }
    const target = await createTarget(targetRequest);
    setValidations(current => {
      const next = { ...current };
      delete next[id];
      return next;
    });
    setEditingTargetId('');
    setEditingTargetHadValidationError(false);
    setEditingTargetPreserveApiKeyRef(false);
    setEditingTargetPreserveApiKeyEnvRef(false);
    setEditingTargetPendingAutoBenchmarkPackId('');
    const validation = await validateSavedTarget(target.id);
    await finishModelTargetHandoff(
      target,
      validation,
      name,
      wasEditing,
      null,
      null,
      wasRepairingValidationError,
      { runPendingAutoBenchmarkPackId: pendingAutoBenchmarkPackId },
    );
  }

  async function finishModelTargetHandoff(
    target: Target,
    validation: TargetValidation | null,
    name: string,
    wasEditing: boolean,
    prestartedJob: RunJob | null = null,
    benchmarkError: string | null = null,
    wasRepairingValidationError = false,
    options: { pendingAutoBenchmarkPackId?: string; pendingAutoBenchmarkPricingTargetIds?: string[]; runPendingAutoBenchmarkPackId?: string } = {},
  ) {
    await onRefresh();
    if (!validation) {
      setMessage(`Target ${name} saved, but validation could not run`);
      return;
    }
    if (validation.status === 'error') {
      setMessage(`Target ${name} saved, but validation failed: ${validation.detail}`);
      return;
    }
    const validationNote = validation.status === 'ok' ? 'validated' : `saved with warning: ${validation.detail}`;
    if (wasEditing) {
      if (options.runPendingAutoBenchmarkPackId && (target.kind === 'direct_model' || target.kind === 'harnessed_model')) {
        const packId = options.runPendingAutoBenchmarkPackId;
        const packLabel = benchmarkPackLabel(packId, modelBenchmarkPacks);
        const targetUniverse = targetListWithOverride(target, targets);
        const intent = automaticModelBenchmarkIntentForTarget(target, targetUniverse, packId, autoBenchmarkTargetIds);
        const scopeLabel = intent.targetIds.length > 1 ? 'local/cloud comparison' : 'benchmark';
        if (cappedIntentHasUnpricedCloudTarget(intent, targetUniverse)) {
          const pricingTargetIds = unpricedCloudTargetIdsForIntent(intent, targetUniverse);
          const repairTarget = firstEditableTargetById(pricingTargetIds, targetUniverse) ?? target;
          setAutoBenchmarkTargetIds(intent.targetIds);
          await loadTargetForEdit(repairTarget, { pendingAutoBenchmarkPackId: packId });
          const repairNames = targetLabelsById(pricingTargetIds, targetUniverse);
          setMessage(`Target ${name} ${validationNote}. Add input/output pricing for ${previewList(repairNames)} before BenchForge starts a capped ${packLabel} ${scopeLabel}.`);
          return;
        }
        try {
          const job = await startRunJob(
            intent.targetIds,
            false,
            packId,
            intent.repetitions ?? 1,
            intent.warmupRuns ?? 0,
            intent.concurrency ?? 1,
            intent.maxCostUsd,
          );
          setAutoBenchmarkTargetIds([]);
          await onRefresh();
          if (!isJobActive(job) && job.results.length) {
            openResultsForGroup(job.runGroupId, job.results[0]?.id);
            setMessage(`Updated target ${name}; ${validationNote}. Capped ${packLabel} ${scopeLabel} completed with ${job.results.length} result(s)`);
            return;
          }
          openRunBuilder(intent);
          setMessage(`Updated target ${name}; ${validationNote}. Queued capped ${packLabel} ${scopeLabel} job ${job.id.slice(0, 8)}`);
          return;
        } catch (error) {
          setAutoBenchmarkTargetIds([]);
          openRunBuilder(intent);
          setMessage(`Updated target ${name}; ${validationNote}, but the automatic ${packLabel} ${scopeLabel} job could not start: ${benchmarkRunFailureMessage(error)}`);
          return;
        }
      }
      if (wasRepairingValidationError && (target.kind === 'direct_model' || target.kind === 'harnessed_model')) {
        const comparisonPackId = recommendedComparisonPackId(packs);
        const comparisonIntent = modelComparisonIntentForTarget(target, targets, comparisonPackId, { requirePricedCloud: true });
        if (comparisonIntent) {
          setAutoBenchmarkTargetIds([]);
          openRunBuilder(comparisonIntent);
          setMessage(`Updated target ${name}; ${validationNote}. Run Builder is ready to rerun the local/cloud ${benchmarkPackLabel(comparisonPackId, modelBenchmarkPacks)} comparison with 3 repetitions, 1 warmup, and ${formatCost(defaultComparisonMaxCostUsd)} cap`);
          return;
        }
      }
      setAutoBenchmarkTargetIds([]);
      setMessage(`Updated target ${name}; ${validationNote}`);
      return;
    }
    const packId = options.pendingAutoBenchmarkPackId || autoBenchmarkPackId || connectivityBenchmarkPackId;
    const packLabel = benchmarkPackLabel(packId, modelBenchmarkPacks);
    const targetUniverse = targetListWithOverride(target, targets);
    const intent = automaticModelBenchmarkIntentForTarget(target, targetUniverse, packId, autoBenchmarkTargetIds);
    const scopeLabel = intent.targetIds.length > 1 ? 'local/cloud comparison' : 'benchmark';
    if (options.pendingAutoBenchmarkPackId) {
      const pricingTargetIds = options.pendingAutoBenchmarkPricingTargetIds?.length
        ? options.pendingAutoBenchmarkPricingTargetIds
        : unpricedCloudTargetIdsForIntent(intent, targetUniverse);
      const repairTarget = firstEditableTargetById(pricingTargetIds, targetUniverse) ?? target;
      setAutoBenchmarkTargetIds(intent.targetIds);
      await loadTargetForEdit(repairTarget, { pendingAutoBenchmarkPackId: options.pendingAutoBenchmarkPackId });
      const repairNames = targetLabelsById(pricingTargetIds, targetUniverse);
      setMessage(`Target ${name} ${validationNote}. Add input/output pricing for ${previewList(repairNames)} before BenchForge starts a capped ${packLabel} ${scopeLabel}.`);
      return;
    }
    if (!autoBenchmarkAfterAdd) {
      setAutoBenchmarkTargetIds([]);
      openRunBuilder(intent);
      setMessage(`Target ${name} ${validationNote} and ready in Runs`);
      return;
    }
    if (benchmarkError) {
      setAutoBenchmarkTargetIds([]);
      openRunBuilder(intent);
      setMessage(`Target ${name} ${validationNote}, but the automatic ${packLabel} ${scopeLabel} job could not start: ${benchmarkRunFailureMessage(benchmarkError)}`);
      return;
    }
    if (prestartedJob) {
      setAutoBenchmarkTargetIds([]);
      await onRefresh();
      if (!isJobActive(prestartedJob) && prestartedJob.results.length) {
        openResultsForGroup(prestartedJob.runGroupId, prestartedJob.results[0]?.id);
        setMessage(`Target ${name} ${validationNote}; capped ${packLabel} ${scopeLabel} completed with ${prestartedJob.results.length} result(s)`);
        return;
      }
      openRunBuilder(intent);
      setMessage(`Target ${name} ${validationNote}; queued capped ${packLabel} ${scopeLabel} job ${prestartedJob.id.slice(0, 8)}`);
      return;
    }
    try {
      const job = await startRunJob(
        intent.targetIds,
        false,
        packId,
        intent.repetitions ?? 1,
        intent.warmupRuns ?? 0,
        intent.concurrency ?? 1,
        intent.maxCostUsd,
      );
      setAutoBenchmarkTargetIds([]);
      await onRefresh();
      if (!isJobActive(job) && job.results.length) {
        openResultsForGroup(job.runGroupId, job.results[0]?.id);
        setMessage(`Target ${name} ${validationNote}; capped ${packLabel} ${scopeLabel} completed with ${job.results.length} result(s)`);
        return;
      }
      openRunBuilder(intent);
      setMessage(`Target ${name} ${validationNote}; queued capped ${packLabel} ${scopeLabel} job ${job.id.slice(0, 8)}`);
    } catch (error) {
      setAutoBenchmarkTargetIds([]);
      openRunBuilder(intent);
      setMessage(`Target ${name} ${validationNote}, but the automatic ${packLabel} ${scopeLabel} job could not start: ${benchmarkRunFailureMessage(error)}`);
    }
  }

  async function validateOne(id: string) {
    setValidating(id);
    try {
      const result = await validateTarget(id);
      setValidations(current => ({ ...current, [id]: result }));
      await onRefresh();
      setMessage(`${id}: ${result.status}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setValidating('');
    }
  }

  async function validateAll() {
    setValidating('all');
    try {
      const activeTargets = targets.filter(target => target.enabled !== false);
      if (!activeTargets.length) {
        setMessage('No enabled targets to validate');
        return;
      }
      const results = await Promise.all(activeTargets.map(target => validateTarget(target.id)));
      setValidations(Object.fromEntries(results.map(result => [result.targetId, result])));
      await onRefresh();
      setMessage(validationSummary(results));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setValidating('');
    }
  }

  async function removeTarget(target: Target) {
    if (target.id === 'mock-agent') {
      setMessage('Mock target is built in and cannot be deleted');
      return;
    }
    if (!window.confirm(`Delete target "${target.name}"? Historical benchmark results will be kept.`)) {
      return;
    }
    const deleted = await deleteTarget(target.id);
    if (!deleted) {
      setMessage(`Target ${target.id} was not deleted`);
      return;
    }
    setValidations(current => {
      const next = { ...current };
      delete next[target.id];
      return next;
    });
    if (editingTargetId === target.id) {
      clearTargetForm();
    }
    if (editingHarnessTargetId === target.id) {
      clearHarnessForm();
    }
    await onRefresh();
    setMessage(`Deleted target ${target.name}`);
  }

  async function copyTargetConfig(target: Target) {
    const exported = await exportTargetRedacted(target.id);
    const text = JSON.stringify(exported, null, 2);
    await navigator.clipboard.writeText(text);
    setMessage(`Copied redacted target config for ${target.name}`);
  }

  async function duplicateExistingTarget(target: Target) {
    if (target.id === 'mock-agent') {
      setMessage('Mock target is built in and does not need a duplicate');
      return;
    }
    setDuplicatingTargetId(target.id);
    try {
      const duplicated = await duplicateTarget(target.id);
      setValidations(current => {
        const next = { ...current };
        delete next[duplicated.id];
        return next;
      });
      await onRefresh();
      if (duplicated.kind === 'direct_model') {
        await loadTargetForEdit(duplicated);
        setMessage(`Duplicated ${target.name} as ${duplicated.name}. Edit the clone, then save and validate before comparing.`);
        return;
      }
      if (duplicated.kind === 'benchmark_harness') {
        await loadHarnessForEdit(duplicated);
        setMessage(`Duplicated ${target.name} as ${duplicated.name}. Edit the harness clone, then save and validate before running.`);
        return;
      }
      setMessage(`Duplicated ${target.name} as ${duplicated.name}`);
    } finally {
      setDuplicatingTargetId('');
    }
  }

  async function toggleTargetEnabled(target: Target) {
    const nextEnabled = target.enabled === false;
    if (!nextEnabled && target.id === 'mock-agent') {
      setMessage('Mock target is built in and cannot be disabled');
      return;
    }
    const updated = await setTargetEnabled(target.id, nextEnabled);
    setValidations(current => {
      const next = { ...current };
      delete next[target.id];
      return next;
    });
    if (!nextEnabled) {
      if (editingTargetId === target.id) {
        clearTargetForm();
      }
      if (editingHarnessTargetId === target.id) {
        clearHarnessForm();
      }
    }
    await onRefresh();
    setMessage(`${nextEnabled ? 'Enabled' : 'Disabled'} target ${updated.name}`);
  }

  function comparisonIntentForTarget(target: Target): RunBuilderIntent | null {
    return modelComparisonIntentForTarget(target, targets, comparisonPackId, { requirePricedCloud: true });
  }

  function comparisonPricingRepairTarget(target: Target) {
    if (!targetIsSelectableModel(target)) {
      return null;
    }
    if (isCloudModelTarget(target)) {
      return localComparisonTargetIds.length && !targetHasInputOutputPricing(target) ? target : null;
    }
    if (isLocalModelTarget(target) && !pricedCloudComparisonTargets.length) {
      return unpricedCloudComparisonTargets[0] ?? null;
    }
    return null;
  }

  async function openPricingRepairForComparison(target: Target) {
    await loadTargetForEdit(target);
    setMessage(`Add input/output pricing for ${target.name} before opening a capped local/cloud comparison.`);
  }

  function openComparisonForTarget(target: Target) {
    const intent = comparisonIntentForTarget(target);
    if (!intent) {
      const missingSide = isLocalModelTarget(target) ? 'a cloud' : isCloudModelTarget(target) ? 'a local' : 'a local or cloud';
      setMessage(`Add ${missingSide} model target before comparing ${target.name}`);
      return;
    }
    openRunBuilder(intent);
    const intentTargets = intent.targetIds
      .map(id => targets.find(candidate => candidate.id === id))
      .filter((candidate): candidate is Target => Boolean(candidate));
    const unpricedCloudTargetIds = intentTargets
      .filter(candidate => isCloudModelTarget(candidate) && !targetHasInputOutputPricing(candidate))
      .map(candidate => candidate.id);
    const pricingNote = unpricedCloudTargetIds.length
      ? ` Add pricing for cloud target(s) before running with a max-cost cap: ${previewList(unpricedCloudTargetIds)}.`
      : ' Priced cloud target selected for capped cost estimates.';
    setMessage(`Run Builder ready to compare ${intent.targetIds.length} local/cloud target(s) with ${benchmarkPackLabel(intent.benchmarkPackId ?? comparisonPackId, modelBenchmarkPacks)}.${pricingNote}`);
  }

  async function openAllLocalCloudComparison() {
    if (!localComparisonTargetIds.length || !cloudComparisonTargets.length) {
      setMessage('Add at least one enabled local model target and one enabled cloud model target before comparing');
      return;
    }
    if (!pricedCloudComparisonTargetIds.length) {
      await openPricingRepairForComparison(cloudComparisonTargets[0]);
      return;
    }
    openRunBuilder(localCloudRunBuilderIntent(allComparisonTargetIds, comparisonPackId));
    const pricingNote = skippedUnpricedCloudTargetIds.length
      ? ` Skipped unpriced cloud target(s): ${previewList(skippedUnpricedCloudTargetIds)}.`
      : ' Selected priced cloud target(s) for cost-capped comparison.';
    setMessage(`Run Builder ready to compare ${localComparisonTargetIds.length} local and ${pricedCloudComparisonTargetIds.length} cloud target(s) with ${benchmarkPackLabel(comparisonPackId, modelBenchmarkPacks)}.${pricingNote}`);
  }

  function plannedModelTargetFromForm() {
    if (!selectedAdapter || !model.trim()) {
      return null;
    }
    const parsedInputPrice = parseOptionalNonNegativeNumber(inputPrice, 'Input price');
    const parsedOutputPrice = parseOptionalNonNegativeNumber(outputPrice, 'Output price');
    const parsedCacheReadPrice = parseOptionalNonNegativeNumber(cacheReadPrice, 'Cache read price');
    const parsedCacheWritePrice = parseOptionalNonNegativeNumber(cacheWritePrice, 'Cache write price');
    if (parsedInputPrice.error || parsedOutputPrice.error || parsedCacheReadPrice.error || parsedCacheWritePrice.error) {
      return null;
    }
    const name = targetName.trim() || `${selectedAdapter.name} ${model.trim()}`;
    const id = editingTargetId || slugify(`${selectedAdapter.id}-${model.trim()}`);
    return plannedModelTarget(id, name, selectedAdapter.id, baseUrl, {
      inputPriceUsdPerMillionTokens: parsedInputPrice.value,
      outputPriceUsdPerMillionTokens: parsedOutputPrice.value,
      cacheReadPriceUsdPerMillionTokens: parsedCacheReadPrice.value,
      cacheWritePriceUsdPerMillionTokens: parsedCacheWritePrice.value,
    });
  }

  function modelTargetAddActionLabel() {
    if (editingTargetId) {
      if (editingTargetPendingAutoBenchmarkPackId) {
        const plannedTarget = plannedModelTargetFromForm();
        if (!plannedTarget) {
          return 'Update target';
        }
        const targetUniverse = targetListWithOverride(plannedTarget, targets);
        const intent = automaticModelBenchmarkIntentForTarget(
          plannedTarget,
          targetUniverse,
          editingTargetPendingAutoBenchmarkPackId,
          autoBenchmarkTargetIds,
        );
        if (!cappedIntentHasUnpricedCloudTarget(intent, targetUniverse)) {
          return intent.targetIds.length > 1 ? 'Update + compare' : 'Update + run';
        }
      }
      return 'Update target';
    }
    if (!autoBenchmarkAfterAdd) {
      return 'Add target';
    }
    const plannedTarget = plannedModelTargetFromForm();
    if (!plannedTarget) {
      return 'Add target';
    }
    const packId = autoBenchmarkPackId || connectivityBenchmarkPackId;
    const targetUniverse = targetListWithOverride(plannedTarget, targets);
    const intent = automaticModelBenchmarkIntentForTarget(plannedTarget, targetUniverse, packId, autoBenchmarkTargetIds);
    if (cappedIntentHasUnpricedCloudTarget(intent, targetUniverse)) {
      return 'Add + price';
    }
    return intent.targetIds.length > 1 ? 'Add + compare' : 'Add + run';
  }

  const providerKeyState = providerKeyStatus(needsApiKey, providerKeyStatusBusy, providerKeyAvailable, apiKey, apiKeyEnv, providerKeyDetail);
  const modelTargetActionLabel = modelTargetAddActionLabel();
  const plannedAutoBenchmarkTarget = plannedModelTargetFromForm();
  const autoBenchmarkPreviewPackId = autoBenchmarkPackId || connectivityBenchmarkPackId;
  const autoBenchmarkPreviewUniverse = plannedAutoBenchmarkTarget
    ? targetListWithOverride(plannedAutoBenchmarkTarget, targets)
    : targets;
  const autoBenchmarkPreviewIntent = plannedAutoBenchmarkTarget && autoBenchmarkAfterAdd && !editingTargetId
    ? automaticModelBenchmarkIntentForTarget(
        plannedAutoBenchmarkTarget,
        autoBenchmarkPreviewUniverse,
        autoBenchmarkPreviewPackId,
        autoBenchmarkTargetIds,
      )
    : null;
  const autoBenchmarkPreviewNeedsPricing = autoBenchmarkPreviewIntent
    ? cappedIntentHasUnpricedCloudTarget(autoBenchmarkPreviewIntent, autoBenchmarkPreviewUniverse)
    : false;
  const autoBenchmarkPreviewUnpricedCloudIds = autoBenchmarkPreviewIntent
    ? unpricedCloudTargetIdsForIntent(autoBenchmarkPreviewIntent, autoBenchmarkPreviewUniverse)
    : [];
  const pendingAutomaticBenchmarkPackId = editingTargetPendingAutoBenchmarkPackId;
  const pendingAutomaticBenchmarkTarget = plannedModelTargetFromForm();
  const pendingAutomaticBenchmarkUniverse = pendingAutomaticBenchmarkTarget
    ? targetListWithOverride(pendingAutomaticBenchmarkTarget, targets)
    : targets;
  const pendingAutomaticBenchmarkIntent = pendingAutomaticBenchmarkPackId && pendingAutomaticBenchmarkTarget
    ? automaticModelBenchmarkIntentForTarget(
        pendingAutomaticBenchmarkTarget,
        pendingAutomaticBenchmarkUniverse,
        pendingAutomaticBenchmarkPackId,
        autoBenchmarkTargetIds,
      )
    : null;
  const pendingAutomaticBenchmarkUnpricedCloudIds = pendingAutomaticBenchmarkIntent
    ? unpricedCloudTargetIdsForIntent(pendingAutomaticBenchmarkIntent, pendingAutomaticBenchmarkUniverse)
    : [];
  const pendingAutomaticBenchmarkNeedsPricing = pendingAutomaticBenchmarkIntent
    ? cappedIntentHasUnpricedCloudTarget(pendingAutomaticBenchmarkIntent, pendingAutomaticBenchmarkUniverse)
    : false;

  return <section><div className="section-head"><h1>Targets</h1><div className="actions"><button disabled={validating === 'all'} onClick={() => validateAll().catch(error => setMessage(String(error)))}><RefreshCw size={16} />Validate all</button><label className="compact-select">Compare pack <select value={comparisonPackId} onChange={event => setComparisonPackId(event.target.value)}>{modelBenchmarkPacks.map(pack => <option key={pack.id} value={pack.id}>{pack.label}</option>)}</select></label><button disabled={comparisonActionDisabled} title={comparisonActionDisabled ? 'Add one enabled local model target and one enabled cloud model target' : comparisonNeedsCloudPricing ? 'Add input/output pricing to a cloud target before opening a capped comparison' : `Compare enabled local targets with ${pricedCloudComparisonTargetIds.length} priced cloud target(s)`} onClick={() => openAllLocalCloudComparison().catch(error => setMessage(String(error)))}>{comparisonNeedsCloudPricing ? <Pencil size={16} /> : <ClipboardCheck size={16} />}{comparisonNeedsCloudPricing ? 'Add cloud pricing' : 'Compare local/cloud'}</button><button onClick={() => addMock().catch(error => setMessage(String(error)))}><Plus size={16} />Mock target</button><button onClick={() => addWorkerHarness().catch(error => setMessage(String(error)))}><Plus size={16} />Worker harness</button></div></div>
    <div className="panel compact">
      <div className="form-title"><h2>Model Target</h2>{editingTargetId ? <span className="mini-tag">Editing {editingTarget?.name ?? editingTargetId}</span> : null}</div>
      <div className="form-grid">
        {!editingTargetId && autoBenchmarkAfterAdd && autoBenchmarkTargetIds.length ? <TargetSetupPlanPanel
          benchmarkPackId={autoBenchmarkPreviewPackId}
          packLabel={benchmarkPackLabel(autoBenchmarkPreviewPackId, modelBenchmarkPacks)}
          targetIds={autoBenchmarkTargetIds}
          targets={targets}
        /> : null}
        <label>Adapter <select value={adapterId} onChange={event => {
          selectAdapter(event.target.value);
        }}>{runnableAdapters.map(adapter => <option key={adapter.id} value={adapter.id}>{adapter.name}</option>)}</select></label>
        <label>Model preset <select value={modelPresetId} onChange={event => applyModelPreset(event.target.value)} disabled={!modelPresets.length}><option value="custom">Custom model</option>{modelPresets.map(preset => <option key={preset.id} value={preset.id}>{preset.label}</option>)}</select></label>
        <label>Model <input value={model} onChange={event => handleModelChange(event.target.value)} placeholder={selectedAdapter?.id === 'azure-openai' ? 'Azure deployment name' : 'gpt-4.1-mini, claude-sonnet-4-5, qwen2.5-coder'} /></label>
        {baseUrlIsPrimary ? <label>Base URL <input value={baseUrl} onChange={event => setBaseUrl(event.target.value)} placeholder={selectedAdapter?.defaultBaseUrl ?? 'optional'} /></label> : null}
        <label>API key <input ref={apiKeyInputRef} type="password" value={apiKey} onChange={event => setApiKey(event.target.value)} placeholder={editingTargetId ? 'leave blank to keep saved key' : needsApiKey ? 'saved to Keychain' : 'optional for local endpoints'} /></label>
        <div className="key-status"><span className={`pill ${providerKeyState.className}`}>{providerKeyState.label}</span><span>{providerKeyState.detail}</span>{needsApiKey ? <button type="button" disabled={providerKeyStatusBusy} onClick={() => refreshSelectedProviderKeyStatus().catch(error => setMessage(String(error)))}><RefreshCw size={14} /></button> : null}</div>
        <details className="advanced-section" open={targetAdvancedOpen} onToggle={event => setTargetAdvancedOpen(event.currentTarget.open)}>
          <summary><SlidersHorizontal size={14} />Advanced</summary>
          <div className="form-grid">
            <label>Name <input value={targetName} onChange={event => setTargetName(event.target.value)} placeholder="optional display name" /></label>
            {!baseUrlIsPrimary ? <label>Base URL <input value={baseUrl} onChange={event => setBaseUrl(event.target.value)} placeholder={selectedAdapter?.defaultBaseUrl ?? 'optional'} /></label> : null}
            <label>API key env <input value={apiKeyEnv} onChange={event => setApiKeyEnv(event.target.value)} placeholder={editingTargetPreserveApiKeyEnvRef ? 'leave blank to keep env ref' : 'optional, e.g. OPENAI_API_KEY'} /></label>
            <label>Temperature <input type="number" min="0" max="2" step="0.1" value={temperature} onChange={event => setTemperature(event.target.value)} placeholder="0" /></label>
            <label>Top P <input type="number" min="0" max="1" step="0.05" value={topP} onChange={event => setTopP(event.target.value)} placeholder="1" /></label>
            <label>Max tokens <input type="number" min="1" step="1" value={maxTokens} onChange={event => setMaxTokens(event.target.value)} placeholder="512" /></label>
            <label>Seed <input type="number" step="1" value={seed} onChange={event => setSeed(event.target.value)} placeholder="optional" /></label>
            <label>Timeout sec <input type="number" min="1" step="1" value={timeoutSeconds} onChange={event => setTimeoutSeconds(event.target.value)} placeholder="120" /></label>
            <label>Retries <input type="number" min="0" max="5" step="1" value={retryCount} onChange={event => setRetryCount(event.target.value)} placeholder="1" /></label>
            <label>Input $/1M tok <input type="number" min="0" step="0.000001" value={inputPrice} onChange={event => setInputPrice(event.target.value)} placeholder="optional" /></label>
            <label>Output $/1M tok <input type="number" min="0" step="0.000001" value={outputPrice} onChange={event => setOutputPrice(event.target.value)} placeholder="optional" /></label>
            <label>Cache read $/1M tok <input type="number" min="0" step="0.000001" value={cacheReadPrice} onChange={event => setCacheReadPrice(event.target.value)} placeholder="optional" /></label>
            <label>Cache write $/1M tok <input type="number" min="0" step="0.000001" value={cacheWritePrice} onChange={event => setCacheWritePrice(event.target.value)} placeholder="optional" /></label>
          </div>
        </details>
        {!editingTargetId ? <label className="toggle"><input type="checkbox" checked={autoBenchmarkAfterAdd} onChange={event => setAutoBenchmarkAfterAdd(event.target.checked)} />Run benchmark after add</label> : null}
        {!editingTargetId ? <label>Benchmark pack <select value={autoBenchmarkPackId} disabled={!autoBenchmarkAfterAdd} onChange={event => setAutoBenchmarkPackId(event.target.value)}>{modelBenchmarkPacks.map(pack => <option key={pack.id} value={pack.id}>{pack.label}</option>)}</select></label> : null}
        {!editingTargetId ? <AutomaticBenchmarkPreview
          enabled={autoBenchmarkAfterAdd}
          plannedTarget={plannedAutoBenchmarkTarget}
          intent={autoBenchmarkPreviewIntent}
          targets={autoBenchmarkPreviewUniverse}
          packLabel={benchmarkPackLabel(autoBenchmarkPreviewPackId, modelBenchmarkPacks)}
          needsPricing={autoBenchmarkPreviewNeedsPricing}
          unpricedCloudTargetIds={autoBenchmarkPreviewUnpricedCloudIds}
        /> : null}
        {editingTargetId && pendingAutomaticBenchmarkPackId ? <PendingAutomaticBenchmarkPanel
          plannedTarget={pendingAutomaticBenchmarkTarget}
          intent={pendingAutomaticBenchmarkIntent}
          targets={pendingAutomaticBenchmarkUniverse}
          packLabel={benchmarkPackLabel(pendingAutomaticBenchmarkPackId, modelBenchmarkPacks)}
          needsPricing={pendingAutomaticBenchmarkNeedsPricing}
          unpricedCloudTargetIds={pendingAutomaticBenchmarkUnpricedCloudIds}
        /> : null}
        <div className="form-actions"><button onClick={() => addModelTarget().catch(error => setMessage(String(error)))}>{editingTargetId ? <Pencil size={16} /> : <Plus size={16} />}{modelTargetActionLabel}</button>{editingTargetId ? <button onClick={() => clearTargetForm()}><RotateCcw size={16} />Cancel</button> : null}</div>
      </div>
      <div className="browser-controls">
        <input value={cloudModelQuery} onChange={event => setCloudModelQuery(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') searchModels().catch(error => setMessage(String(error))); }} placeholder={`Search ${selectedAdapter?.name ?? 'cloud'} models`} />
        <button disabled={!selectedAdapter || cloudModelBusy} onClick={() => searchModels().catch(error => setMessage(String(error)))}><Search size={16} />{cloudModelBusy ? 'Searching' : 'Search'}</button>
      </div>
      {cloudModels.length ? <div className="model-browser">{cloudModels.map(item => {
        const catalogIntent = cloudCatalogBenchmarkIntent(item);
        const catalogPlannedTarget = cloudCatalogPlannedTarget(item);
        const catalogUniverse = catalogPlannedTarget ? targetListWithOverride(catalogPlannedTarget, targets) : targets;
        const catalogNeedsPricing = Boolean(catalogIntent && cappedIntentHasUnpricedCloudTarget(catalogIntent, catalogUniverse));
        const catalogNeedsKey = cloudCatalogNeedsKeyBeforeAdd();
        const catalogActionTitle = editingTargetId
          ? 'Finish or cancel the current edit before adding a catalog model'
          : catalogNeedsKey
            ? `Paste an API key for ${selectedAdapter?.name ?? item.provider} before adding this cloud target`
            : catalogNeedsPricing
            ? 'Save and validate this cloud target, then add pricing before the capped comparison can run'
            : 'Save, validate, and use the current automatic benchmark setting';
        const catalogActionIcon = catalogNeedsKey || catalogNeedsPricing ? <Pencil size={16} /> : <ClipboardCheck size={16} />;
        return <div className="model-row" key={`${item.source}-${item.model}`}>
          <div className="model-main">
            <strong>{item.name}</strong>
            <span className="muted">{item.model}</span>
            <div className="tag-row">
              <span className="mini-tag">{item.provider}</span>
              <span className="mini-tag">{formatPricePair(item.inputPriceUsdPerMillionTokens, item.outputPriceUsdPerMillionTokens, item.cacheReadPriceUsdPerMillionTokens, item.cacheWritePriceUsdPerMillionTokens)}</span>
              {item.contextLength ? <span className="mini-tag">{formatInteger(item.contextLength)} ctx</span> : null}
              <span className="mini-tag">{item.source}</span>
            </div>
            {item.detail ? <span className="muted">{item.detail}</span> : null}
          </div>
          <div className="model-actions">
            <button disabled={Boolean(catalogAddBusyModel) || Boolean(editingTargetId)} title={catalogActionTitle} onClick={() => addCloudModelFromCatalog(item).catch(error => setMessage(String(error)))}>{catalogActionIcon}{catalogAddBusyModel === `${item.source}:${item.model}` ? 'Adding' : cloudCatalogActionLabel(item)}</button>
            <button disabled={Boolean(catalogAddBusyModel)} onClick={() => useCloudModel(item)}><Plus size={16} />Use</button>
          </div>
        </div>;
      })}</div> : null}
      <p className="muted">Cloud keys are saved to macOS Keychain. Generation settings and optional pricing are stored with the target for reproducible comparisons.</p>
      {selectedModelPreset?.note ? <p className="muted">{selectedModelPreset.note}</p> : null}
      {typeof selectedAdapter?.metadata?.pricing_note === 'string' ? <p className="muted">{selectedAdapter.metadata.pricing_note}</p> : null}
      {typeof selectedAdapter?.metadata?.setup_note === 'string' ? <p className="muted">{selectedAdapter.metadata.setup_note}</p> : null}
    </div>
    <div className="panel compact">
      <div className="form-title"><h2>Worker Harness Target</h2>{editingHarnessTargetId ? <span className="mini-tag">Editing {editingHarnessTarget?.name ?? editingHarnessTargetId}</span> : null}</div>
      <div className="form-grid">
        <label>Preset <select value={harnessPresetId} onChange={event => applyHarnessPreset(event.target.value)}><option value="custom">Custom command</option>{harnessPresets.map(preset => <option key={preset.id} value={preset.id}>{preset.label}</option>)}</select></label>
        <label>Target ID <input value={harnessTargetId} disabled={Boolean(editingHarnessTargetId)} onChange={event => setHarnessTargetId(event.target.value)} placeholder="evalplus-local" /></label>
        <label>Name <input value={harnessTargetName} onChange={event => setHarnessTargetName(event.target.value)} placeholder="EvalPlus local harness" /></label>
        <label>Model / run arg <input value={harnessModel} onChange={event => setHarnessModel(event.target.value)} placeholder={selectedHarnessPreset?.modelPlaceholder ?? 'optional, available as {model}'} /></label>
        <label>Base URL <input value={harnessBaseUrl} onChange={event => setHarnessBaseUrl(event.target.value)} placeholder={selectedHarnessPreset?.baseUrlPlaceholder ?? 'optional, available as {base_url}'} /></label>
        <label className="span-two">Harness command <input value={harnessCommand} onChange={event => setHarnessCommand(event.target.value)} placeholder="python3 -m evalplus.evaluate --dataset {dataset} --samples {workspace}/samples.jsonl" /></label>
        <label>Timeout sec <input type="number" min="1" step="1" value={harnessTimeoutSeconds} onChange={event => setHarnessTimeoutSeconds(event.target.value)} placeholder="3600" /></label>
        <label className="span-two">Env passthrough <input value={harnessEnvPassthrough} onChange={event => setHarnessEnvPassthrough(event.target.value)} placeholder="OPENAI_API_KEY, ANTHROPIC_API_KEY" /></label>
        <label className="span-two">Setup notes <textarea rows={3} value={harnessSetupHint} onChange={event => setHarnessSetupHint(event.target.value)} placeholder="Install/check notes stored with this harness target" /></label>
        <div className="form-actions"><button onClick={() => addOrUpdateWorkerHarness().catch(error => setMessage(String(error)))}>{editingHarnessTargetId ? <Pencil size={16} /> : <Plus size={16} />}{editingHarnessTargetId ? 'Update harness' : 'Add harness'}</button>{editingHarnessTargetId ? <button onClick={() => clearHarnessForm()}><RotateCcw size={16} />Cancel</button> : null}</div>
      </div>
      {selectedHarnessPreset ? <div className="setup-guide">
        <div className="tag-row">{selectedHarnessPreset.tags.map(tag => <span className="mini-tag" key={`${selectedHarnessPreset.id}-${tag}`}>{tag}</span>)}</div>
        <div className="setup-guide-row"><strong>Install</strong><code>{selectedHarnessPreset.installCommand}</code></div>
        <div className="setup-guide-row"><strong>Check</strong><code>{selectedHarnessPreset.checkCommand}</code></div>
        <div className="actions">
          <button disabled={Boolean(harnessToolBusy)} onClick={() => runSelectedHarnessTool('check').catch(error => setMessage(String(error)))}><Search size={14} />{harnessToolBusy === 'check' ? 'Checking' : 'Check tool'}</button>
          <button disabled={Boolean(harnessToolBusy)} onClick={() => runSelectedHarnessTool('install').catch(error => setMessage(String(error)))}><Wrench size={14} />{harnessToolBusy === 'install' ? 'Installing' : 'Install tool'}</button>
        </div>
        {harnessToolResult ? <div>
          <div className="tag-row"><span className={`pill ${harnessToolResult.status === 'ready' ? 'ok' : harnessToolResult.status === 'error' ? 'error' : 'warn'}`}>{harnessToolResult.status}</span><span className="mini-tag">{harnessToolResult.action}</span></div>
          <pre className="setup-log">{harnessToolResult.log}</pre>
        </div> : null}
        <p className="muted">{selectedHarnessPreset.outputHint}</p>
      </div> : null}
      <p className="muted">Use the command your external harness would run locally. BenchForge Worker captures scores, errors, and raw harness output as run artifacts.</p>
    </div>
    <div className="panel compact">
      <div className="section-head"><h2>Local Runtimes</h2><div className="actions"><span className={`pill ${localRuntimeDoctorCheck.check.status}`} title={localRuntimeDoctorCheck.check.detail}>{localRuntimeDoctorCheck.note}</span><button disabled={detectingLocal} onClick={() => { autoLocalRuntimeDetectAttemptedRef.current = true; detectLocal().catch(error => setMessage(String(error))); }}><Search size={16} />{detectingLocal ? 'Detecting' : 'Detect'}</button></div></div>
      <table><thead><tr><th>Runtime</th><th>Endpoint</th><th>Status</th><th>Model</th><th></th></tr></thead><tbody>{localRuntimes.map(runtime => {
        const selectedModel = localSelections[runtime.id] ?? runtime.recommendedModel ?? runtime.models[0] ?? '';
        const runtimeResult = runtimeToolResult?.runtimeId === runtime.id ? runtimeToolResult : null;
        const runtimeCanAdd = localRuntimeCanAddTarget(runtime, selectedModel);
        const runtimePlannedTarget = runtimeCanAdd ? localRuntimePlannedTarget(runtime, selectedModel) : null;
        const runtimePreviewPackId = autoBenchmarkPackId || connectivityBenchmarkPackId;
        const runtimePreviewUniverse = runtimePlannedTarget ? targetListWithOverride(runtimePlannedTarget, targets) : targets;
        const runtimePreviewIntent = runtimePlannedTarget && autoBenchmarkAfterAdd
          ? automaticModelBenchmarkIntentForTarget(runtimePlannedTarget, runtimePreviewUniverse, runtimePreviewPackId, autoBenchmarkTargetIds)
          : null;
        const runtimePreviewNeedsPricing = runtimePreviewIntent
          ? cappedIntentHasUnpricedCloudTarget(runtimePreviewIntent, runtimePreviewUniverse)
          : false;
        const runtimePreviewUnpricedCloudIds = runtimePreviewIntent
          ? unpricedCloudTargetIdsForIntent(runtimePreviewIntent, runtimePreviewUniverse)
          : [];
        const runtimeActionTitle = !runtimeCanAdd
          ? 'Start the local runtime and select a model before adding it'
          : runtimePreviewNeedsPricing
            ? 'Save and validate this local target, then add cloud pricing before the capped comparison can run'
            : 'Save, validate, and use the current automatic benchmark setting';
        return <tr key={runtime.id}><td>{runtime.name}</td><td>{runtime.baseUrl}</td><td><span className={`pill ${runtime.status === 'ok' ? 'ok' : runtime.status === 'error' ? 'error' : 'warn'}`}>{runtime.status}</span> {runtime.detail}
          {runtime.modelSource ? <div className="tag-row"><span className="mini-tag" title={runtime.probeUrl ?? undefined}>{runtime.modelSource}</span></div> : null}
          {runtime.setupHint ? <div className="muted">{runtime.setupHint}</div> : null}
          <div className="runtime-hints">
            {runtime.installCommand ? <code>{runtime.installCommand}</code> : null}
            {runtime.startCommand ? <code>{runtime.startCommand}</code> : null}
          </div>
          {localRuntimeToolSupported(runtime.id) ? <div className="actions">
            <button disabled={Boolean(runtimeToolBusy)} onClick={() => runSelectedLocalRuntimeTool(runtime, 'check').catch(error => setMessage(String(error)))}><Search size={14} />{runtimeToolBusy === `${runtime.id}:check` ? 'Checking' : 'Check tool'}</button>
            <button disabled={Boolean(runtimeToolBusy)} onClick={() => runSelectedLocalRuntimeTool(runtime, 'install').catch(error => setMessage(String(error)))}><Wrench size={14} />{runtimeToolBusy === `${runtime.id}:install` ? 'Installing' : 'Install tool'}</button>
            {runtime.id === 'ollama' ? <button disabled={Boolean(runtimeToolBusy) || !selectedModel.trim()} onClick={() => runSelectedLocalRuntimeTool(runtime, 'pull', selectedModel.trim()).catch(error => setMessage(String(error)))}><Download size={14} />{runtimeToolBusy === `${runtime.id}:pull` ? 'Pulling' : 'Pull model'}</button> : null}
          </div> : null}
          {runtimeResult ? <div><div className="tag-row"><span className={`pill ${runtimeResult.status === 'ready' ? 'ok' : runtimeResult.status === 'error' ? 'error' : 'warn'}`}>{runtimeResult.status}</span><span className="mini-tag">{runtimeResult.action}</span></div><pre className="setup-log">{runtimeResult.log}</pre></div> : null}
        </td><td>{runtime.models.length ? <select value={selectedModel} onChange={event => setLocalSelections(current => ({ ...current, [runtime.id]: event.target.value }))}>{runtime.models.map(modelName => <option key={modelName} value={modelName}>{modelName}</option>)}</select> : <input value={selectedModel} onChange={event => setLocalSelections(current => ({ ...current, [runtime.id]: event.target.value }))} placeholder={runtime.modelHint ?? 'model id'} />}</td><td><button disabled={Boolean(addingLocalRuntimeId) || !runtimeCanAdd} title={runtimeActionTitle} onClick={() => addDetectedRuntime(runtime, selectedModel).catch(error => setMessage(String(error)))}>{runtimePreviewNeedsPricing ? <Pencil size={16} /> : <ClipboardCheck size={16} />}{addingLocalRuntimeId === runtime.id ? 'Adding' : localRuntimeActionLabel(runtime, selectedModel)}</button><AutomaticBenchmarkInlinePreview
          enabled={autoBenchmarkAfterAdd}
          plannedTarget={runtimePlannedTarget}
          intent={runtimePreviewIntent}
          targets={runtimePreviewUniverse}
          packLabel={benchmarkPackLabel(runtimePreviewPackId, modelBenchmarkPacks)}
          needsPricing={runtimePreviewNeedsPricing}
          unpricedCloudTargetIds={runtimePreviewUnpricedCloudIds}
        /></td></tr>;
      })}</tbody></table>
      {!localRuntimes.length && <p className="muted">Detect Ollama, LM Studio, llama.cpp, vLLM, MLX / mlx-lm, and oMLX OpenAI-compatible servers on their default local ports.</p>}
    </div>
    <table><thead><tr><th>Name</th><th>Model</th><th>Endpoint / CLI</th><th>Kind</th><th>Adapter</th><th>Stored</th><th>Health</th><th></th></tr></thead><tbody>{targets.map(t => {
      const validation = validations[t.id] ?? targetValidationFromTarget(t);
      const editable = t.kind === 'direct_model' || t.kind === 'benchmark_harness';
      const comparisonIntent = comparisonIntentForTarget(t);
      const pricingRepairTarget = comparisonPricingRepairTarget(t);
      const comparableModel = t.kind === 'direct_model' || t.kind === 'harnessed_model';
      const runnable = targetIsSelectableForRun(t);
      const targetEnabled = t.enabled !== false;
      const storedStatus = targetEnabled ? t.status : 'disabled';
      const storedStatusClass = targetEnabled ? t.status : 'warn';
      const endpointDisplay = targetEndpointDisplay(t);
      return <tr key={t.id}><td>{t.name}</td><td><span className="target-identity">{t.model || '-'}</span></td><td><span className="target-identity">{endpointDisplay || '-'}</span></td><td>{t.kind}<TargetModelTags target={t} /></td><td>{t.adapterId}</td><td><span className={`pill ${storedStatusClass}`}>{storedStatus}</span></td><td>{validation ? <><span className={`pill ${validation.status === 'ok' ? 'ok' : validation.status === 'error' ? 'error' : 'warn'}`}>{validation.status}</span> {validation.detail}{validation.checkedAt ? <div className="muted">Checked {formatDateTime(validation.checkedAt)}</div> : null}</> : <span className="muted">{targetEnabled ? 'not checked' : 'disabled'}</span>}</td><td><div className="row-actions"><button disabled={!runnable} title={runnable ? 'Open Run Builder with this target' : 'Enable and validate this target before running it'} onClick={() => { openRunBuilder(runBuilderIntentForTarget(t)); setMessage(`Run Builder ready for ${t.name}`); }}><Play size={14} />Run</button>{comparableModel ? <button disabled={!comparisonIntent && !pricingRepairTarget} title={comparisonIntent ? 'Compare this target against the first available priced local/cloud counterpart' : pricingRepairTarget ? 'Add input/output pricing before opening a capped local/cloud comparison' : 'Add an enabled target from the other side before comparing'} onClick={() => { if (comparisonIntent) { openComparisonForTarget(t); return; } if (pricingRepairTarget) { openPricingRepairForComparison(pricingRepairTarget).catch(error => setMessage(String(error))); } }}>{pricingRepairTarget && !comparisonIntent ? <Pencil size={14} /> : <ClipboardCheck size={14} />}{pricingRepairTarget && !comparisonIntent ? 'Pricing' : 'Compare'}</button> : null}<button disabled={Boolean(loadingTargetId) || !editable || !targetEnabled} title={!targetEnabled ? 'Enable target before editing it' : editable ? 'Edit target' : 'This target type is not editable here'} onClick={() => { if (t.kind === 'benchmark_harness') { loadHarnessForEdit(t).catch(error => setMessage(String(error))); } else { loadTargetForEdit(t).catch(error => setMessage(String(error))); } }}><Pencil size={14} />{loadingTargetId === t.id ? 'Loading' : 'Edit'}</button><button disabled={Boolean(loadingTargetId) || Boolean(duplicatingTargetId) || t.id === 'mock-agent'} title={t.id === 'mock-agent' ? 'Built-in target' : 'Clone this target with the same safe configuration'} onClick={() => duplicateExistingTarget(t).catch(error => setMessage(String(error)))}><Copy size={14} />{duplicatingTargetId === t.id ? 'Duplicating' : 'Duplicate'}</button><button disabled={Boolean(loadingTargetId)} title="Copy redacted target JSON without secrets" onClick={() => copyTargetConfig(t).catch(error => setMessage(String(error)))}><Copy size={14} />Config</button><button disabled={Boolean(validating) || !targetEnabled} onClick={() => validateOne(t.id).catch(error => setMessage(String(error)))}>{validating === t.id ? 'Checking' : 'Validate'}</button><button disabled={Boolean(validating) || t.id === 'mock-agent'} title={t.id === 'mock-agent' ? 'Built-in target' : targetEnabled ? 'Disable target without deleting history' : 'Enable target'} onClick={() => toggleTargetEnabled(t).catch(error => setMessage(String(error)))}>{targetEnabled ? <Square size={14} /> : <Play size={14} />}{targetEnabled ? 'Disable' : 'Enable'}</button><button disabled={Boolean(validating) || t.id === 'mock-agent'} title={t.id === 'mock-agent' ? 'Built-in target' : 'Delete target permanently'} onClick={() => removeTarget(t).catch(error => setMessage(String(error)))}><Trash2 size={14} />Delete</button></div></td></tr>;
    })}</tbody></table>
    <h2>Adapters</h2><table><thead><tr><th>Name</th><th>Kind</th><th>Command / Endpoint</th><th>Validation</th></tr></thead><tbody>{adapters.map(adapter => <tr key={adapter.id}><td>{adapter.name}</td><td>{adapter.kind}</td><td>{adapter.command ?? adapter.defaultBaseUrl ?? '-'}</td><td><span className={`pill ${adapter.validationStatus}`}>{adapter.validationStatus}</span> {adapter.validationDetail}</td></tr>)}</tbody></table>
  </section>;
}

function Benchmarks({ packs, diagnostics, onRefresh, setMessage }: { packs: BenchmarkPack[]; diagnostics: BenchmarkPackDiagnostic[]; onRefresh: () => Promise<void>; setMessage: (message: string) => void }) {
  const [packName, setPackName] = useState('My Private Eval');
  const [packId, setPackId] = useState('my-private-eval');
  const [description, setDescription] = useState('Private prompt checks for my workload.');
  const [prompt, setPrompt] = useState('Reply with exactly OK.');
  const [expectedResponse, setExpectedResponse] = useState('OK');
  const [creating, setCreating] = useState(false);
  const [selectedTaskPackId, setSelectedTaskPackId] = useState('');
  const [taskName, setTaskName] = useState('Private prompt check');
  const [taskId, setTaskId] = useState('');
  const [taskPrompt, setTaskPrompt] = useState('Answer in one sentence and include the words local and cloud.');
  const [taskScoringMethod, setTaskScoringMethod] = useState('contains');
  const [taskExpectedResponse, setTaskExpectedResponse] = useState('local\ncloud');
  const [taskTimeout, setTaskTimeout] = useState('120');
  const [taskWeight, setTaskWeight] = useState('1');
  const [addingTask, setAddingTask] = useState(false);
  const [editingTaskId, setEditingTaskId] = useState('');
  const [taskSampleResponse, setTaskSampleResponse] = useState('This local benchmark can compare cloud models.');
  const [previewingScorer, setPreviewingScorer] = useState(false);
  const [scorerPreview, setScorerPreview] = useState<ScorePromptTaskPreview | null>(null);
  const [previewPackId, setPreviewPackId] = useState('');
  const [previewTasks, setPreviewTasks] = useState<BenchmarkPackTask[]>([]);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [deletingTaskId, setDeletingTaskId] = useState('');
  const [transferPackId, setTransferPackId] = useState('');
  const [exportDestinationDir, setExportDestinationDir] = useState('');
  const [exportFormat, setExportFormat] = useState<'folder' | 'zip'>('folder');
  const [importSourcePath, setImportSourcePath] = useState('');
  const [exportingPackId, setExportingPackId] = useState('');
  const [importingPack, setImportingPack] = useState(false);
  const [calibrationPackId, setCalibrationPackId] = useState('');
  const [calibrationStatus, setCalibrationStatus] = useState('uncalibrated');
  const [calibrationSampleSize, setCalibrationSampleSize] = useState('');
  const [calibrationBaselineModels, setCalibrationBaselineModels] = useState('');
  const [calibrationLastReviewed, setCalibrationLastReviewed] = useState('');
  const [calibrationNotes, setCalibrationNotes] = useState('');
  const [savingCalibration, setSavingCalibration] = useState(false);
  const [suggestingCalibration, setSuggestingCalibration] = useState(false);
  const [calibrationSuggestionWarnings, setCalibrationSuggestionWarnings] = useState<string[]>([]);
  const invalidDiagnostics = diagnostics.filter(diagnostic => diagnostic.status !== 'ok');
  const userPacks = packs.filter(pack => pack.source === 'user');
  const calibrationPack = userPacks.find(pack => pack.id === calibrationPackId);
  const previewPack = packs.find(pack => pack.id === previewPackId);
  const canDeletePreviewTasks = previewPack?.source === 'user' && previewTasks.length > 1;
  const sanitizedPackId = slugifyInput(packId);
  const canCreate = Boolean(packName.trim() && sanitizedPackId && prompt.trim() && expectedResponse.trim() && !creating);
  const sanitizedTaskId = slugifyInput(taskId);
  const taskExpectationRequired = scoringMethodRequiresExpected(taskScoringMethod);
  const taskExpectedPlaceholder = scoringExpectedPlaceholder(taskScoringMethod);
  const parsedTaskTimeout = Number(taskTimeout);
  const parsedTaskWeight = taskWeight.trim() ? Number(taskWeight) : undefined;
  const taskNumbersValid = Number.isInteger(parsedTaskTimeout) && parsedTaskTimeout >= 1 && parsedTaskTimeout <= 3600
    && (parsedTaskWeight == null || (Number.isFinite(parsedTaskWeight) && parsedTaskWeight > 0 && parsedTaskWeight <= 100));
  const canAddTask = Boolean(userPacks.length && selectedTaskPackId && taskName.trim() && taskPrompt.trim() && taskNumbersValid && (!taskExpectationRequired || taskExpectedResponse.trim()) && !addingTask);
  const canPreviewScorer = Boolean((!taskExpectationRequired || taskExpectedResponse.trim()) && !previewingScorer);
  const canExportPack = Boolean(transferPackId && !exportingPackId);
  const canImportPack = Boolean(importSourcePath.trim() && !importingPack);
  const parsedCalibrationSampleSize = calibrationSampleSize.trim() ? Number(calibrationSampleSize) : undefined;
  const calibrationSampleValid = parsedCalibrationSampleSize == null || (Number.isInteger(parsedCalibrationSampleSize) && parsedCalibrationSampleSize >= 0);
  const parsedCalibrationBaselineModels = calibrationBaselineModels.split(/\r?\n|,/).map(model => model.trim()).filter(Boolean);
  const calibrationProvenanceError = calibrationStatus === 'calibrated'
    ? parsedCalibrationSampleSize == null || parsedCalibrationSampleSize <= 0
      ? 'Calibrated packs need a positive sample size.'
      : parsedCalibrationBaselineModels.length < 2
        ? 'Calibrated packs need at least two baseline models.'
        : !calibrationLastReviewed.trim()
          ? 'Calibrated packs need a review date.'
          : !calibrationNotes.trim()
            ? 'Calibrated packs need review notes.'
            : ''
    : '';
  const canSaveCalibration = Boolean(calibrationPackId && calibrationSampleValid && !calibrationProvenanceError && !savingCalibration && !suggestingCalibration);
  const canSuggestCalibration = Boolean(calibrationPackId && !savingCalibration && !suggestingCalibration);

  useEffect(() => {
    if (!userPacks.length) {
      if (selectedTaskPackId) {
        setSelectedTaskPackId('');
      }
      return;
    }
    if (!selectedTaskPackId || !userPacks.some(pack => pack.id === selectedTaskPackId)) {
      setSelectedTaskPackId(userPacks[0].id);
    }
  }, [packs, selectedTaskPackId]);

  useEffect(() => {
    if (!userPacks.length) {
      if (calibrationPackId) {
        setCalibrationPackId('');
      }
      return;
    }
    if (!calibrationPackId || !userPacks.some(pack => pack.id === calibrationPackId)) {
      setCalibrationPackId(userPacks[0].id);
    }
  }, [packs, calibrationPackId]);

  useEffect(() => {
    setCalibrationSuggestionWarnings([]);
    if (!calibrationPack) {
      setCalibrationStatus('uncalibrated');
      setCalibrationSampleSize('');
      setCalibrationBaselineModels('');
      setCalibrationLastReviewed('');
      setCalibrationNotes('');
      return;
    }
    setCalibrationStatus(knownCalibrationStatus(calibrationPack.calibrationStatus));
    setCalibrationSampleSize(calibrationPack.calibrationSampleSize == null ? '' : String(calibrationPack.calibrationSampleSize));
    setCalibrationBaselineModels(calibrationPack.calibrationBaselineModels.join('\n'));
    setCalibrationLastReviewed(calibrationPack.calibrationLastReviewed ?? '');
    setCalibrationNotes(calibrationPack.calibrationNotes ?? '');
  }, [
    calibrationPack?.id,
    calibrationPack?.calibrationStatus,
    calibrationPack?.calibrationSampleSize,
    calibrationPack?.calibrationBaselineModels.join('\n'),
    calibrationPack?.calibrationLastReviewed,
    calibrationPack?.calibrationNotes,
  ]);

  useEffect(() => {
    if (!packs.length) {
      if (previewPackId) {
        setPreviewPackId('');
      }
      setPreviewTasks([]);
      return;
    }
    if (!previewPackId || !packs.some(pack => pack.id === previewPackId)) {
      setPreviewPackId(packs[0].id);
    }
  }, [packs, previewPackId]);

  useEffect(() => {
    if (!packs.length) {
      if (transferPackId) {
        setTransferPackId('');
      }
      return;
    }
    if (!transferPackId || !packs.some(pack => pack.id === transferPackId)) {
      setTransferPackId(packs[0].id);
    }
  }, [packs, transferPackId]);

  useEffect(() => {
    if (!previewPackId) {
      setPreviewTasks([]);
      return;
    }
    let cancelled = false;
    setPreviewLoading(true);
    listBenchmarkPackTasks(previewPackId)
      .then(tasks => {
        if (!cancelled) {
          setPreviewTasks(tasks);
        }
      })
      .catch(error => {
        if (!cancelled) {
          setPreviewTasks([]);
          setMessage(String(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setPreviewLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [previewPackId, packs]);

  useEffect(() => {
    setScorerPreview(null);
  }, [taskScoringMethod, taskExpectedResponse, taskSampleResponse]);

  function applyPackName(value: string) {
    const previousSlug = slugifyInput(packName);
    setPackName(value);
    if (!packId.trim() || packId === previousSlug) {
      setPackId(slugifyInput(value));
    }
  }

  async function createTemplate() {
    if (!canCreate) {
      return;
    }
    setCreating(true);
    try {
      const created = await createBenchmarkPackTemplate({
        id: sanitizedPackId,
        name: packName.trim(),
        description: description.trim() || undefined,
        prompt: prompt.trim(),
        expectedResponse: expectedResponse.trim(),
      });
      setSelectedTaskPackId(created.pack.id);
      await onRefresh();
      setMessage(`Created benchmark pack ${created.pack.id}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setCreating(false);
    }
  }

  function applyTaskName(value: string) {
    const previousSlug = slugifyInput(taskName);
    setTaskName(value);
    if (editingTaskId) {
      return;
    }
    if (!taskId.trim() || taskId === previousSlug) {
      setTaskId(slugifyInput(value));
    }
  }

  function applyTaskScoringMethod(value: string) {
    setTaskScoringMethod(value);
    if (value === 'contains' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('local\ncloud');
    } else if (value === 'exact' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('OK');
    } else if (value === 'regex' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('(?i)benchmark');
    } else if (value === 'json_field_equals' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"status":"ok","allowed":true}');
    } else if (value === 'json_field_contains' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"summary":["local","cloud"]}');
    } else if (value === 'json_field_array_exact' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"evidence_ids":["A1","A2"]}');
    } else if (value === 'json_field_array_exact_ordered' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"steps":["validate","run","export"]}');
    } else if (value === 'json_field_object_keys_exact' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"$":["decision","reason","cost_usd"]}');
    } else if (value === 'json_field_number_close' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"total_cost_usd":{"expected":0.05,"tolerance":0.001}}');
    } else if (value === 'json_field_number_bounds' && !taskExpectedResponse.trim()) {
      setTaskExpectedResponse('{"latency_ms":{"min":0,"max":5000}}');
    }
  }

  async function addPromptTask() {
    if (!canAddTask) {
      return;
    }
    setAddingTask(true);
    try {
      const request = {
        packId: selectedTaskPackId,
        taskId: editingTaskId || sanitizedTaskId || undefined,
        name: taskName.trim(),
        prompt: taskPrompt.trim(),
        scoringMethod: taskScoringMethod,
        expectedResponse: taskExpectationRequired ? taskExpectedResponse.trim() : undefined,
        timeoutSeconds: parsedTaskTimeout,
        weight: parsedTaskWeight,
      };
      const result = editingTaskId
        ? await updateBenchmarkPackPromptTask({ ...request, taskId: editingTaskId })
        : await addBenchmarkPackPromptTask(request);
      setPreviewPackId(result.pack.id);
      setEditingTaskId('');
      if (!editingTaskId) {
        setTaskId('');
      }
      await onRefresh();
      const refreshedTasks = await listBenchmarkPackTasks(result.pack.id);
      setPreviewTasks(refreshedTasks);
      setMessage(`${editingTaskId ? 'Updated' : 'Added'} task ${result.taskId} in ${result.pack.id}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setAddingTask(false);
    }
  }

  async function previewPromptScorer() {
    if (!canPreviewScorer) {
      return;
    }
    setPreviewingScorer(true);
    try {
      const preview = await scorePromptTaskPreview({
        scoringMethod: taskScoringMethod,
        expectedResponse: taskExpectationRequired ? taskExpectedResponse.trim() : undefined,
        sampleResponse: taskSampleResponse,
      });
      setScorerPreview(preview);
      setMessage(`Scorer preview ${preview.status} with score ${preview.score.toFixed(2)}`);
    } catch (error) {
      setScorerPreview(null);
      setMessage(String(error));
    } finally {
      setPreviewingScorer(false);
    }
  }

  function loadTaskForEdit(task: BenchmarkPackTask) {
    if (!previewPack || previewPack.source !== 'user' || !taskEditableByPromptForm(task)) {
      setMessage('This task uses scoring that the prompt-task editor cannot safely rewrite');
      return;
    }
    const editor = scoringEditorFromTask(task);
    setSelectedTaskPackId(previewPack.id);
    setTaskName(task.name);
    setTaskId(task.id);
    setEditingTaskId(task.id);
    setTaskPrompt(task.prompt);
    setTaskScoringMethod(editor.method);
    setTaskExpectedResponse(editor.expected);
    setTaskTimeout(String(task.timeoutSeconds || 120));
    setTaskWeight(String(task.weight || 1));
    setScorerPreview(null);
    setMessage(`Loaded ${task.id} for editing`);
  }

  function cancelTaskEdit() {
    setEditingTaskId('');
    setTaskId('');
    setTaskName('Private prompt check');
    setTaskPrompt('Answer in one sentence and include the words local and cloud.');
    setTaskScoringMethod('contains');
    setTaskExpectedResponse('local\ncloud');
    setTaskTimeout('120');
    setTaskWeight('1');
    setScorerPreview(null);
  }

  async function deletePreviewTask(task: BenchmarkPackTask) {
    if (!previewPack || previewPack.source !== 'user') {
      return;
    }
    if (!window.confirm(`Delete task ${task.id} from ${previewPack.name}?`)) {
      return;
    }
    setDeletingTaskId(task.id);
    try {
      const deleted = await deleteBenchmarkPackTask({ packId: previewPack.id, taskId: task.id });
      setPreviewTasks(current => current.filter(item => item.id !== deleted.deletedTaskId));
      await onRefresh();
      setMessage(`Deleted task ${deleted.deletedTaskId} from ${deleted.pack.id}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setDeletingTaskId('');
    }
  }

  async function exportSelectedPack() {
    if (!canExportPack) {
      return;
    }
    setExportingPackId(transferPackId);
    try {
      const exported = await exportBenchmarkPack({
        packId: transferPackId,
        destinationDir: exportDestinationDir.trim() || undefined,
        format: exportFormat,
      });
      setMessage(`Exported ${exported.pack.id} ${exported.format === 'zip' ? 'zip' : 'folder'} to ${exported.exportPath} (${exported.filesCopied} file(s))`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setExportingPackId('');
    }
  }

  async function importPackFolder() {
    if (!canImportPack) {
      return;
    }
    setImportingPack(true);
    try {
      const imported = await importBenchmarkPack({ sourcePath: importSourcePath.trim() });
      await onRefresh();
      setSelectedTaskPackId(imported.pack.id);
      setPreviewPackId(imported.pack.id);
      setTransferPackId(imported.pack.id);
      setImportSourcePath('');
      setMessage(`Imported ${imported.pack.id} to ${imported.importPath} (${imported.filesCopied} file(s))`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setImportingPack(false);
    }
  }

  async function suggestPackCalibration() {
    if (!canSuggestCalibration) {
      return;
    }
    setSuggestingCalibration(true);
    setCalibrationSuggestionWarnings([]);
    try {
      const suggestion = await suggestBenchmarkPackCalibration({ packId: calibrationPackId });
      setCalibrationStatus(knownCalibrationStatus(suggestion.status));
      setCalibrationSampleSize(String(suggestion.sampleSize));
      setCalibrationBaselineModels(suggestion.baselineModels.join('\n'));
      setCalibrationLastReviewed(suggestion.lastReviewed ?? '');
      setCalibrationNotes(suggestion.notes);
      setCalibrationSuggestionWarnings(suggestion.warnings);
      setMessage(`Suggested calibration for ${suggestion.packId} from ${suggestion.sampleSize} evidence row(s). Review and save to update the pack.`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setSuggestingCalibration(false);
    }
  }

  async function savePackCalibration() {
    if (!canSaveCalibration) {
      return;
    }
    setSavingCalibration(true);
    try {
      const updated = await updateBenchmarkPackCalibration({
        packId: calibrationPackId,
        status: calibrationStatus,
        sampleSize: parsedCalibrationSampleSize,
        baselineModels: parsedCalibrationBaselineModels,
        lastReviewed: calibrationLastReviewed.trim() || undefined,
        notes: calibrationNotes.trim() || undefined,
      });
      await onRefresh();
      setMessage(`Updated calibration for ${updated.pack.id}: ${formatCalibrationStatus(updated.pack.calibrationStatus)}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setSavingCalibration(false);
    }
  }

  return <section><h1>Benchmark Packs</h1>
    <div className="panel">
      <div className="panel-head"><h2>Private Pack Template</h2><button disabled={!canCreate} onClick={() => createTemplate().catch(error => setMessage(String(error)))}><Plus size={14} />{creating ? 'Creating' : 'Create'}</button></div>
      <div className="form-grid">
        <label>Name <input value={packName} onChange={event => applyPackName(event.target.value)} /></label>
        <label>ID <input value={packId} onChange={event => setPackId(slugifyInput(event.target.value))} placeholder="my-private-eval" /></label>
        <label className="span-two">Description <input value={description} onChange={event => setDescription(event.target.value)} /></label>
        <label className="span-two">Prompt <textarea rows={4} value={prompt} onChange={event => setPrompt(event.target.value)} /></label>
        <label>Expected response <input value={expectedResponse} onChange={event => setExpectedResponse(event.target.value)} /></label>
        <label>Path <input readOnly value={`.benchforge/benchmark-packs/${sanitizedPackId || 'pack-id'}`} /></label>
      </div>
    </div>
    <div className="panel">
      <div className="panel-head"><h2>{editingTaskId ? 'Edit Prompt Task' : 'Prompt Task'}</h2><div className="actions">{editingTaskId ? <button disabled={addingTask} onClick={cancelTaskEdit}><Square size={14} />Cancel</button> : null}<button disabled={!canAddTask} onClick={() => addPromptTask().catch(error => setMessage(String(error)))}><Plus size={14} />{addingTask ? (editingTaskId ? 'Saving' : 'Adding') : (editingTaskId ? 'Save task' : 'Add task')}</button></div></div>
      <div className="form-grid">
        <label>Pack <select value={selectedTaskPackId} disabled={!userPacks.length || Boolean(editingTaskId)} onChange={event => setSelectedTaskPackId(event.target.value)}>
          {userPacks.length ? userPacks.map(pack => <option key={pack.id} value={pack.id}>{pack.name}</option>) : <option value="">Create a private pack first</option>}
        </select></label>
        <label>Task ID <input value={taskId} disabled={Boolean(editingTaskId)} onChange={event => setTaskId(slugifyInput(event.target.value))} placeholder="auto-generated" /></label>
        <label className="span-two">Task name <input value={taskName} onChange={event => applyTaskName(event.target.value)} /></label>
        <label className="span-two">Prompt <textarea rows={4} value={taskPrompt} onChange={event => setTaskPrompt(event.target.value)} /></label>
        <label>Scoring <select value={taskScoringMethod} onChange={event => applyTaskScoringMethod(event.target.value)}>
          <option value="contains">Contains all</option>
          <option value="exact">Exact</option>
          <option value="json">Valid JSON</option>
          <option value="regex">Regex</option>
          <option value="json_field_equals">JSON fields equal</option>
          <option value="json_field_contains">JSON fields contain</option>
          <option value="json_field_array_exact">JSON array exact</option>
          <option value="json_field_array_exact_ordered">JSON array ordered</option>
          <option value="json_field_object_keys_exact">JSON object keys</option>
          <option value="json_field_number_close">JSON number close</option>
          <option value="json_field_number_bounds">JSON number bounds</option>
          <option value="non_empty">Non-empty</option>
        </select></label>
        <label>Timeout seconds <input value={taskTimeout} onChange={event => setTaskTimeout(event.target.value.replace(/[^0-9]/g, ''))} /></label>
        <label>Weight <input value={taskWeight} onChange={event => setTaskWeight(event.target.value.replace(/[^0-9.]/g, ''))} /></label>
        {taskExpectationRequired ? <label className="span-two">Expected <textarea rows={3} value={taskExpectedResponse} onChange={event => setTaskExpectedResponse(event.target.value)} placeholder={taskExpectedPlaceholder} /></label> : null}
        <label className="span-two">Sample response <textarea rows={3} value={taskSampleResponse} onChange={event => setTaskSampleResponse(event.target.value)} placeholder="Paste a model response to test this scorer before running a benchmark" /></label>
      </div>
      <div className="row-actions"><button disabled={!canPreviewScorer} onClick={() => previewPromptScorer().catch(error => setMessage(String(error)))}><ClipboardCheck size={14} />{previewingScorer ? 'Testing' : 'Test scorer'}</button>{scorerPreview ? <span className={`pill ${scorerPreview.status === 'passed' ? 'ok' : 'error'}`}>{scorerPreview.status} {scorerPreview.score.toFixed(2)}</span> : null}{scorerPreview?.errorMessage ? <span className="muted">{scorerPreview.errorMessage}</span> : null}</div>
      {scorerPreview ? <pre className="scorer-preview">{JSON.stringify(scorerPreview.tests, null, 2)}</pre> : null}
    </div>
    <div className="panel">
      <div className="panel-head"><h2>Pack Calibration</h2><div className="actions"><button disabled={!canSuggestCalibration} onClick={() => suggestPackCalibration().catch(error => setMessage(String(error)))}><ClipboardCheck size={14} />{suggestingCalibration ? 'Suggesting' : 'Suggest from results'}</button><button disabled={!canSaveCalibration} onClick={() => savePackCalibration().catch(error => setMessage(String(error)))}><ShieldCheck size={14} />{savingCalibration ? 'Saving' : 'Save'}</button></div></div>
      <div className="form-grid">
        <label>Pack <select value={calibrationPackId} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationPackId(event.target.value)}>
          {userPacks.length ? userPacks.map(pack => <option key={pack.id} value={pack.id}>{pack.name}</option>) : <option value="">Create a private pack first</option>}
        </select></label>
        <label>Status <select value={calibrationStatus} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationStatus(event.target.value)}>
          <option value="uncalibrated">Uncalibrated</option>
          <option value="pilot">Pilot</option>
          <option value="reviewed">Reviewed</option>
          <option value="calibrated">Calibrated</option>
        </select></label>
        <label>Sample size <input value={calibrationSampleSize} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationSampleSize(event.target.value.replace(/[^0-9]/g, ''))} placeholder="0" /></label>
        <label>Last reviewed <input value={calibrationLastReviewed} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationLastReviewed(event.target.value)} placeholder="YYYY-MM-DD" /></label>
        <label className="span-two">Baseline models <textarea rows={3} value={calibrationBaselineModels} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationBaselineModels(event.target.value)} placeholder="One model per line, e.g. local-qwen-7b" /></label>
        <label className="span-two">Notes <textarea rows={3} value={calibrationNotes} disabled={!userPacks.length || savingCalibration || suggestingCalibration} onChange={event => setCalibrationNotes(event.target.value)} placeholder="What was reviewed, sample source, and known limits" /></label>
      </div>
      <p className="muted">{calibrationPack ? packCalibrationSummary(calibrationPack) : 'Create a private pack first.'}</p>
      {calibrationSuggestionWarnings.map(warning => <p key={warning} className="muted">{warning}</p>)}
      {calibrationStatus === 'calibrated' ? <p className="muted">Calibrated packs must include a positive sample size, at least two baseline models, a valid review date, and review notes.</p> : null}
      {calibrationProvenanceError ? <p className="muted">{calibrationProvenanceError}</p> : null}
      {!calibrationSampleValid ? <p className="muted">Sample size must be a whole number.</p> : null}
    </div>
    <div className="panel">
      <div className="panel-head"><h2>Pack Sharing</h2><div className="actions"><button disabled={!canExportPack} onClick={() => exportSelectedPack().catch(error => setMessage(String(error)))}><Download size={14} />{exportingPackId ? 'Exporting' : 'Export'}</button><button disabled={!canImportPack} onClick={() => importPackFolder().catch(error => setMessage(String(error)))}><Upload size={14} />{importingPack ? 'Importing' : 'Import'}</button></div></div>
      <div className="form-grid">
        <label>Export pack <select value={transferPackId} disabled={!packs.length} onChange={event => setTransferPackId(event.target.value)}>
          {packs.length ? packs.map(pack => <option key={pack.id} value={pack.id}>{pack.name}</option>) : <option value="">No packs available</option>}
        </select></label>
        <label>Destination directory <input value={exportDestinationDir} onChange={event => setExportDestinationDir(event.target.value)} placeholder=".benchforge/exports/benchmark-packs" /></label>
        <label>Export format <select value={exportFormat} onChange={event => setExportFormat(event.target.value as 'folder' | 'zip')}><option value="folder">Folder</option><option value="zip">Zip archive</option></select></label>
        <label className="span-two">Import source <input value={importSourcePath} onChange={event => setImportSourcePath(event.target.value)} placeholder="/path/to/pack-folder, pack.yaml, or pack.zip" /></label>
      </div>
    </div>
    <div className="panel">
      <div className="panel-head"><h2>Task Preview</h2><select value={previewPackId} onChange={event => setPreviewPackId(event.target.value)}>
        {packs.map(pack => <option key={pack.id} value={pack.id}>{pack.name}</option>)}
      </select></div>
      {previewLoading ? <p className="muted">Loading tasks...</p> : previewTasks.length ? <table><thead><tr><th>Task</th><th>Prompt</th><th>Scoring</th><th>Settings</th><th>Source</th></tr></thead><tbody>{previewTasks.map(task => <tr key={task.id}>
        <td><strong>{task.name}</strong><div className="muted">{task.id}</div><div className="tag-row"><span className="mini-tag">{task.taskType}</span>{task.language ? <span className="mini-tag">{task.language}</span> : null}</div>{previewPack?.source === 'user' ? <div className="row-actions task-row-actions">{task.taskType === 'prompt' ? <button disabled={addingTask || editingTaskId === task.id || !taskEditableByPromptForm(task)} title={taskEditableByPromptForm(task) ? 'Load task into the editor' : 'This scoring shape is not editable in the form'} onClick={() => loadTaskForEdit(task)}><Pencil size={14} />{editingTaskId === task.id ? 'Editing' : 'Edit'}</button> : null}<button disabled={!canDeletePreviewTasks || deletingTaskId === task.id} title={canDeletePreviewTasks ? 'Delete task' : 'Packs must keep at least one task'} onClick={() => deletePreviewTask(task).catch(error => setMessage(String(error)))}><Trash2 size={14} />{deletingTaskId === task.id ? 'Deleting' : 'Delete'}</button></div> : null}</td>
        <td><div className="task-preview-text">{task.prompt}</div></td>
        <td><div>{formatList(task.scoringMethods)}</div><div className="muted">{formatTaskScoringPreview(task)}</div></td>
        <td><div>{task.timeoutSeconds}s</div><div className="muted">weight {task.weight}</div>{task.maxTurns ? <div className="muted">{task.maxTurns} turns</div> : null}</td>
        <td><code>{task.sourcePath}</code>{task.fixture ? <div className="muted">{task.fixture}</div> : null}</td>
      </tr>)}</tbody></table> : <p className="muted">No tasks loaded.</p>}
    </div>
    {invalidDiagnostics.length ? <div className="panel">
      <div className="panel-head"><h2>Pack Diagnostics</h2><span className="mini-tag">{invalidDiagnostics.length} issue(s)</span></div>
      <table className="compact-table"><thead><tr><th>Status</th><th>Pack</th><th>Source</th><th>Path</th><th>Detail</th></tr></thead><tbody>{invalidDiagnostics.map((diagnostic, index) => <tr key={`${diagnostic.sourcePath}-${index}`}>
        <td><span className={`pill ${diagnostic.status}`}>{diagnostic.status}</span></td>
        <td>{diagnostic.id ?? '-'}</td>
        <td>{diagnostic.source}</td>
        <td><code>{diagnostic.sourcePath}</code></td>
        <td>{diagnostic.detail}</td>
      </tr>)}</tbody></table>
    </div> : null}
    <table><thead><tr><th>Name</th><th>Source</th><th>Runtime</th><th>Fit</th><th>Tasks</th><th>Evidence</th><th>Sandbox</th><th>Tools</th><th>Scoring</th></tr></thead><tbody>{packs.map(pack => <tr key={pack.id}>
    <td><strong>{pack.name}</strong><div className="muted">{pack.description ?? pack.id}</div><div className="tag-row">{pack.tags.slice(0, 5).map(tag => <span className="mini-tag" key={`${pack.id}-${tag}`}>{tag}</span>)}</div></td>
    <td><span className="mini-tag">{pack.source || 'built-in'}</span><div className="muted">{pack.sourcePath || '-'}</div></td>
    <td>{pack.estimatedRuntime ?? '-'}</td>
    <td>{pack.targetFit}<div className="muted">{formatList(pack.supportedTargetKinds)}</div></td>
    <td>{pack.tasks}<div className="muted">{formatList(pack.taskTypes)}</div></td>
    <td><span className="mini-tag">{formatEvidenceProfile(pack.evidenceProfile)}</span><div className="muted">{pack.promptTasks} prompt, weight {formatNumber(pack.totalTaskWeight)}</div><div className="muted">{packCalibrationSummary(pack)}</div>{pack.evidenceWarnings[0] ? <div className="muted">{pack.evidenceWarnings[0]}</div> : null}</td>
    <td><span className={`pill ${pack.requiresSandbox ? 'warn' : 'ok'}`}>{pack.requiresSandbox ? 'required' : 'no'}</span>{pack.heavy ? <div className="mini-tag">heavy</div> : null}</td>
    <td>{formatList(pack.requiredTools)}</td>
    <td>{formatList(pack.scoringMethods)}</td>
  </tr>)}</tbody></table></section>;
}

function Runs({ targets, adapters, packs, busy, setBusy, setMessage, refresh, setPage, openResultsForGroup, openTargetRepair, openTargetSetup, openHuggingFaceLocalSetup, runBuilderIntent, onRunBuilderIntentConsumed }: { targets: Target[]; adapters: Adapter[]; packs: BenchmarkPack[]; busy: boolean; setBusy: (busy: boolean) => void; setMessage: (message: string) => void; refresh: () => Promise<void>; setPage: (page: Page) => void; openResultsForGroup: (groupId: string, runId?: string) => void; openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void; openTargetSetup: (intent: Omit<TargetSetupIntent, 'nonce'>) => void; openHuggingFaceLocalSetup: (intent?: Omit<HuggingFaceLocalSetupIntent, 'nonce'>) => void; runBuilderIntent: RunBuilderIntent | null; onRunBuilderIntentConsumed: () => void }) {
  const [selected, setSelected] = useState<string[]>(['mock-agent']);
  const [selectedPackId, setSelectedPackId] = useState('quick-smoke');
  const [packTasks, setPackTasks] = useState<BenchmarkPackTask[]>([]);
  const [selectedTaskIds, setSelectedTaskIds] = useState<string[]>([]);
  const [pendingTaskIntent, setPendingTaskIntent] = useState<{ packId: string; taskIds: string[] } | null>(null);
  const [taskSelectionOpen, setTaskSelectionOpen] = useState(false);
  const [tasksLoading, setTasksLoading] = useState(false);
  const [taskError, setTaskError] = useState('');
  const [repetitions, setRepetitions] = useState('1');
  const [warmupRuns, setWarmupRuns] = useState('0');
  const [concurrency, setConcurrency] = useState('1');
  const [maxCostUsd, setMaxCostUsd] = useState('');
  const [docker, setDocker] = useState(false);
  const [runAdvancedOpen, setRunAdvancedOpen] = useState(false);
  const [jobs, setJobs] = useState<RunJob[]>([]);
  const [activeJobId, setActiveJobId] = useState('');
  const [runEstimate, setRunEstimate] = useState<RunEstimate | null>(null);
  const [estimateError, setEstimateError] = useState('');
  const [runValidationBlockers, setRunValidationBlockers] = useState<TargetValidation[]>([]);
  const autoSeedRunBuilderDefaultsRef = useRef(false);
  const autoApplyComparisonDefaultsRef = useRef('');
  const activeJob = jobs.find(job => job.id === activeJobId);
  const activeRunInProgress = Boolean(activeJob && isJobActive(activeJob));
  const selectedPack = packs.find(pack => pack.id === selectedPackId);
  const modelBenchmarkPacks = useMemo(() => modelBenchmarkPackOptions(packs), [packs]);
  const setupBenchmarkPackId = resolveModelBenchmarkPackId(selectedPackId, modelBenchmarkPacks, packs);
  const incompatibleSelectedTargets = selectedPack
    ? targets.filter(target => selected.includes(target.id) && !targetCompatibleWithPack(target, selectedPack))
    : [];
  const compatibilityError = selectedPack && incompatibleSelectedTargets.length
    ? `${selectedPack.name} does not support ${incompatibleSelectedTargets.map(target => `${target.name} (${target.kind})`).join(', ')}. Supported target kinds: ${formatList(selectedPack.supportedTargetKinds)}.`
    : '';
  const unavailableSelectedTargets = targets.filter(target => selected.includes(target.id) && !targetIsSelectableForRun(target));
  const unavailableTargetError = unavailableSelectedTargets.length
    ? `Remove or fix unavailable target(s) before running: ${unavailableSelectedTargets.map(target => target.name).join(', ')}.`
    : '';
  const selectedTaskCount = selectedTaskIds.length;
  const taskSelectionError = taskError || (tasksLoading ? 'Loading benchmark tasks...' : selectedPackId && !selectedTaskCount ? 'Select at least one benchmark task.' : '');
  const taskSelectionNeedsAttention = Boolean(taskError || (!tasksLoading && selectedPackId && !selectedTaskCount));
  const directTargets = targets.filter(target => targetIsSelectableModel(target));
  const localTargetIds = directTargets.filter(target => isLocalModelTarget(target)).map(target => target.id);
  const cloudTargets = directTargets.filter(target => isCloudModelTarget(target));
  const cloudSetupAdapterId = usePreferredCloudSetupAdapterId(adapters);
  const cloudTargetIds = cloudTargets.map(target => target.id);
  const pricedCloudTargets = cloudTargets.filter(targetHasInputOutputPricing);
  const unpricedCloudTargetIds = cloudTargets
    .filter(target => !targetHasInputOutputPricing(target))
    .map(target => target.id);
  const preferredCloudTargetIds = (pricedCloudTargets.length ? pricedCloudTargets : cloudTargets).map(target => target.id);
  const skippedUnpricedCloudTargetIds = pricedCloudTargets.length
    ? unpricedCloudTargetIds
    : [];
  const localCloudTargetIds = localTargetIds.length && preferredCloudTargetIds.length
    ? [...localTargetIds, ...preferredCloudTargetIds]
    : [];
  const localCloudSelectedUnpricedCloudTargetIds = localCloudTargetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId));
  const modelTargetIds = directTargets.map(target => target.id);
  const selectedTargets = targets.filter(target => selected.includes(target.id));
  const selectedLocalTargets = selectedTargets.filter(target => isLocalModelTarget(target));
  const selectedCloudTargets = selectedTargets.filter(target => isCloudModelTarget(target));
  const selectedLocalTargetIds = selectedLocalTargets.map(target => target.id);
  const selectedCloudTargetIds = selectedCloudTargets.map(target => target.id);
  const comparisonDefaultsRecommended = Boolean(selectedPack && selectedLocalTargets.length && selectedCloudTargets.length && packUsesModelSelectionDefaults(selectedPack));
  const recommendedComparisonConcurrency = Math.min(2, Math.max(1, selectedTargets.length));
  const selectedPackSupportsModelComparison = Boolean(selectedPack?.taskTypes.includes('prompt') && selectedPack.supportedTargetKinds.includes('direct_model'));
  const canApplyLocalCloudPanelShortcut = Boolean(localCloudTargetIds.length && (selectedLocalTargets.length || selectedCloudTargets.length) && !(selectedLocalTargets.length && selectedCloudTargets.length));
  const canApplyAllComparablePanelShortcut = Boolean(localCloudTargetIds.length
    && selectedLocalTargets.length
    && selectedCloudTargets.length
    && localCloudTargetIds.some(targetId => !selected.includes(targetId)));
  const canOpenLocalSetupFromReadiness = Boolean(selectedCloudTargets.length && !localTargetIds.length);
  const canOpenHfLocalSetupFromReadiness = Boolean(selectedCloudTargets.length && !localTargetIds.length);
  const canOpenCloudSetupFromReadiness = Boolean(selectedLocalTargets.length && !cloudTargetIds.length);
  const localCloudPanelShortcutPackId = selectedPackSupportsModelComparison ? selectedPackId : undefined;
  const parsedRepetitions = parsePositiveIntegerInRange(repetitions, 'Repetitions', 1, 100);
  const parsedWarmupRuns = parsePositiveIntegerInRange(warmupRuns, 'Warmup runs', 0, 20);
  const parsedConcurrency = parsePositiveIntegerInRange(concurrency, 'Concurrency', 1, 8);
  const parsedMaxCostUsd = parseOptionalNonNegativeNumber(maxCostUsd, 'Max cost USD');
  const runAdvancedHasError = Boolean(parsedWarmupRuns.error || parsedConcurrency.error || parsedMaxCostUsd.error);
  const costLimitMessage = runCostLimitMessage(runEstimate, parsedMaxCostUsd.value);
  const blockingEstimateError = blockingRunEstimateErrorMessage(estimateError);
  const runSettingsError = parsedRepetitions.error || parsedWarmupRuns.error || parsedConcurrency.error || parsedMaxCostUsd.error || compatibilityError || unavailableTargetError || taskSelectionError || costLimitMessage || blockingEstimateError;
  const runCostPricingRepairTargetIds = costLimitMessage && runEstimate?.unpricedTargets.length
    ? runEstimate.unpricedTargets
    : [];
  const recommendedRunCostCapUsd = costLimitMessage
    && !runCostPricingRepairTargetIds.length
    && runEstimate?.estimatedMaxCostUsd != null
    && parsedMaxCostUsd.value != null
    && runEstimate.estimatedMaxCostUsd > parsedMaxCostUsd.value
    ? runEstimate.estimatedMaxCostUsd
    : undefined;
  const canAdjustRunCostCap = Boolean(costLimitMessage && !runCostPricingRepairTargetIds.length);
  const estimatedRuns = selected.length * Math.max(1, parsedRepetitions.value ?? 1) * selectedTaskCount;
  const estimatedWarmups = selected.length * (parsedWarmupRuns.value ?? 0);
  const comparisonRunReadiness = selectedPack && (selectedLocalTargets.length || selectedCloudTargets.length)
    ? localCloudRunReadiness({
      pack: selectedPack,
      selectedLocalCount: selectedLocalTargets.length,
      selectedCloudCount: selectedCloudTargets.length,
      availableLocalCount: localTargetIds.length,
      availableCloudCount: cloudTargetIds.length,
      selectedTaskCount,
      totalTaskCount: packTasks.length || selectedPack.tasks,
      repetitions: parsedRepetitions.value,
      warmupRuns: parsedWarmupRuns.value,
      concurrency: parsedConcurrency.value,
      maxCostUsd: parsedMaxCostUsd.value,
      estimate: runEstimate,
    })
    : null;
  const repetitionConfidenceWarning = selectedPack && parsedRepetitions.value != null
    ? runRepetitionConfidenceWarning(selectedPack, parsedRepetitions.value)
    : '';
  const runScaleWarning = runScaleConfidenceWarning({
    measuredRuns: runEstimate?.measuredRuns ?? estimatedRuns,
    warmupCalls: runEstimate?.warmupCalls ?? estimatedWarmups,
    targetCount: selected.length,
    taskCount: selectedTaskCount,
    repetitions: parsedRepetitions.value ?? 1,
    concurrency: parsedConcurrency.value ?? 1,
    wallClockTimeoutSeconds: runEstimate?.estimatedWallClockTimeoutSeconds ?? null,
    heavy: Boolean(selectedPack?.heavy),
  });
  const localCloudShortcutTitle = localCloudShortcutHelp(localCloudTargetIds, localCloudSelectedUnpricedCloudTargetIds, skippedUnpricedCloudTargetIds, targets);
  const localCloudReliabilityShortcutTitle = localCloudShortcutHelp(localCloudTargetIds, localCloudSelectedUnpricedCloudTargetIds, skippedUnpricedCloudTargetIds, targets, 'Reliability');
  const localCloudShortcutLabel = runBuilderComparisonShortcutLabel(localCloudTargetIds, 'Local + cloud');
  const localCloudReadinessShortcutLabel = runBuilderUseComparisonShortcutLabel(localCloudTargetIds);
  const allComparableReadinessShortcutLabel = runBuilderUseAllComparisonShortcutLabel(localCloudTargetIds);
  const allModelsShortcutTitle = targetShortcutHelp('all model targets', modelTargetIds, modelTargetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId)), targets);
  const cloudShortcutTitle = targetShortcutHelp('cloud targets', cloudTargetIds, cloudTargetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId)), targets);

  useEffect(() => {
    setSelected(current => {
      const targetIds = new Set(targets.map(target => target.id));
      const retained = current.filter(id => targetIds.has(id));
      if (retained.length) {
        return retained;
      }
      return targets[0] ? [targets[0].id] : [];
    });
  }, [targets]);

  useEffect(() => {
    if (!packs.length || packs.some(pack => pack.id === selectedPackId)) {
      return;
    }
    setSelectedPackId(packs.some(pack => pack.id === 'quick-smoke') ? 'quick-smoke' : packs[0].id);
  }, [packs, selectedPackId]);

  useEffect(() => {
    if (!selectedPackId) {
      setPackTasks([]);
      setSelectedTaskIds([]);
      setTaskError('');
      setTasksLoading(false);
      return undefined;
    }
    let cancelled = false;
    setTasksLoading(true);
    setTaskError('');
    listBenchmarkPackTasks(selectedPackId)
      .then(tasks => {
        if (cancelled) {
          return;
        }
        setPackTasks(tasks);
        setSelectedTaskIds(tasks.map(task => task.id));
        setTaskError(tasks.length ? '' : 'Selected pack has no runnable tasks.');
      })
      .catch(error => {
        if (cancelled) {
          return;
        }
        setPackTasks([]);
        setSelectedTaskIds([]);
        setTaskError(String(error));
      })
      .finally(() => {
        if (!cancelled) {
          setTasksLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedPackId]);

  useEffect(() => {
    if (!runBuilderIntent || !packs.length) {
      return;
    }
    const availableTargetIds = new Set(targets.map(target => target.id));
    const intendedTargetIds = runBuilderIntent.targetIds.filter(id => availableTargetIds.has(id));
    const missingTargetIds = runBuilderIntent.targetIds.filter(id => !availableTargetIds.has(id));
    if (missingTargetIds.length && !targets.length) {
      return;
    }
    if (!intendedTargetIds.length) {
      setMessage(`Run Builder handoff skipped because target(s) are no longer available: ${missingTargetIds.join(', ')}`);
      onRunBuilderIntentConsumed();
      return;
    }
    setSelected(intendedTargetIds);
    const requestedPackId = runBuilderIntent.benchmarkPackId;
    const nextPackId = requestedPackId && packs.some(pack => pack.id === requestedPackId)
      ? requestedPackId
      : preferredRunPackForTargets(intendedTargetIds, targets, packs);
    if (nextPackId) {
      setSelectedPackId(nextPackId);
    }
    const intendedTaskIds = runBuilderIntent.taskIds?.filter(id => id.trim()) ?? [];
    setPendingTaskIntent(nextPackId && intendedTaskIds.length ? { packId: nextPackId, taskIds: intendedTaskIds } : null);
    const appliedSettings = applyRunBuilderIntentSettings(runBuilderIntent, setRepetitions, setWarmupRuns, setConcurrency, setMaxCostUsd);
    const missingNote = missingTargetIds.length
      ? `Skipped unavailable target(s): ${missingTargetIds.join(', ')}.`
      : '';
    const messageParts = [appliedSettings, missingNote].filter(Boolean);
    if (messageParts.length) {
      setMessage(messageParts.join(' '));
    }
    onRunBuilderIntentConsumed();
  }, [runBuilderIntent, targets, packs, onRunBuilderIntentConsumed, setMessage]);

  useEffect(() => {
    if (autoSeedRunBuilderDefaultsRef.current || runBuilderIntent || !packs.length) {
      return;
    }
    if (selected.length !== 1 || selected[0] !== 'mock-agent' || selectedPackId !== 'quick-smoke' || repetitions !== '1' || warmupRuns !== '0' || concurrency !== '1' || maxCostUsd) {
      return;
    }
    if (localCloudTargetIds.length) {
      const packId = recommendedComparisonPackId(packs);
      if (!packs.some(pack => pack.id === packId)) {
        return;
      }
      const intent = localCloudRunBuilderIntent(localCloudTargetIds, packId);
      autoSeedRunBuilderDefaultsRef.current = true;
      setSelected(intent.targetIds);
      setSelectedPackId(intent.benchmarkPackId ?? packId);
      applyRunBuilderIntentSettings(intent, setRepetitions, setWarmupRuns, setConcurrency, setMaxCostUsd);
      const selectedUnpricedCloudTargetIds = intent.targetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId));
      const pricingNote = selectedUnpricedCloudTargetIds.length
        ? ` Add pricing for ${previewList(targetLabelsById(selectedUnpricedCloudTargetIds, targets))} before running with the cap.`
        : skippedUnpricedCloudTargetIds.length
          ? ` Skipped unpriced cloud target(s): ${previewList(targetLabelsById(skippedUnpricedCloudTargetIds, targets))}.`
          : '';
      setMessage(`Run Builder preselected ${intent.targetIds.length} local/cloud model target(s), ${benchmarkPackLabel(packId)}, 3 repetitions, 1 warmup, ${formatCost(defaultComparisonMaxCostUsd)} cap.${pricingNote}`);
      return;
    }
    if (!modelTargetIds.length) {
      return;
    }
    const packId = preferredRunPackForTargets(modelTargetIds, targets, packs);
    if (!packId || !packs.some(pack => pack.id === packId)) {
      return;
    }
    const settings = automaticModelBenchmarkSettings(packId);
    const includesCloudTarget = modelTargetIds.some(id => cloudTargetIds.includes(id));
    const maxCost = automaticModelBenchmarkMaxCostUsd(packId);
    autoSeedRunBuilderDefaultsRef.current = true;
    setSelected(modelTargetIds);
    setSelectedPackId(packId);
    setRepetitions(String(settings.repetitions));
    setWarmupRuns(String(settings.warmupRuns));
    setConcurrency(String(settings.concurrency));
    if (includesCloudTarget) {
      setMaxCostUsd(String(maxCost));
    }
    const scopeLabel = localTargetIds.length ? 'local model' : 'cloud model';
    const capNote = includesCloudTarget ? `, ${formatCost(maxCost)} cap` : '';
    setMessage(`Run Builder preselected ${modelTargetIds.length} ${scopeLabel} target(s), ${benchmarkPackLabel(packId)}, ${settings.repetitions} repetition(s), ${settings.warmupRuns} warmup(s)${capNote}`);
  }, [runBuilderIntent, packs, localCloudTargetIds, modelTargetIds, targets, cloudTargetIds, localTargetIds, unpricedCloudTargetIds, skippedUnpricedCloudTargetIds, selected, selectedPackId, repetitions, warmupRuns, concurrency, maxCostUsd, setMessage]);

  useEffect(() => {
    if (!pendingTaskIntent || pendingTaskIntent.packId !== selectedPackId || tasksLoading || taskError || !packTasks.length) {
      return;
    }
    const availableTaskIds = new Set(packTasks.map(task => task.id));
    const nextTaskIds = pendingTaskIntent.taskIds.filter(taskId => availableTaskIds.has(taskId));
    setPendingTaskIntent(null);
    if (!nextTaskIds.length) {
      setMessage(`Run Builder skipped requested task subset because none of those task IDs are in ${selectedPackId}`);
      return;
    }
    setTaskSelectionOpen(true);
    setSelectedTaskIds(nextTaskIds);
    setMessage(`Run Builder selected ${nextTaskIds.length} task(s) from coverage follow-up`);
  }, [pendingTaskIntent, selectedPackId, tasksLoading, taskError, packTasks, setMessage]);

  useEffect(() => {
    if (!comparisonDefaultsRecommended || runBuilderIntent) {
      return;
    }
    if (repetitions !== '1' || warmupRuns !== '0' || concurrency !== '1') {
      return;
    }
    const signature = `${selectedPackId}:${selected.slice().sort().join(',')}`;
    if (autoApplyComparisonDefaultsRef.current === signature) {
      return;
    }
    autoApplyComparisonDefaultsRef.current = signature;
    setRepetitions(String(recommendedTaskRepetitions));
    setWarmupRuns('1');
    setConcurrency(String(recommendedComparisonConcurrency));
    if (!maxCostUsd) {
      setMaxCostUsd(String(defaultComparisonMaxCostUsd));
    }
    const existingCap = Number(maxCostUsd);
    const capNote = maxCostUsd
      ? Number.isFinite(existingCap)
        ? `, existing ${formatCost(existingCap)} cap`
        : ', existing custom cap'
      : `, ${formatCost(defaultComparisonMaxCostUsd)} cap`;
    setMessage(`Run Builder applied comparison defaults: ${recommendedTaskRepetitions} repetitions, 1 warmup, concurrency ${recommendedComparisonConcurrency}${capNote}`);
  }, [comparisonDefaultsRecommended, runBuilderIntent, repetitions, warmupRuns, concurrency, maxCostUsd, selectedPackId, selected, recommendedComparisonConcurrency, setMessage]);

  useEffect(() => {
    if (!selected.length || !selectedPackId || !selectedTaskIds.length || tasksLoading || taskError || compatibilityError || unavailableTargetError || parsedRepetitions.error || parsedWarmupRuns.error || parsedConcurrency.error || parsedRepetitions.value == null || parsedWarmupRuns.value == null || parsedConcurrency.value == null) {
      setRunEstimate(null);
      setEstimateError('');
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      estimateRunPlan(selected, selectedPackId, parsedRepetitions.value ?? 1, parsedWarmupRuns.value ?? 0, parsedConcurrency.value ?? 1, selectedTaskIds)
        .then(estimate => {
          if (!cancelled) {
            setRunEstimate(estimate);
            setEstimateError('');
          }
        })
        .catch(error => {
          if (!cancelled) {
            setRunEstimate(null);
            setEstimateError(String(error));
          }
        });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [selected, selectedPackId, selectedTaskIds, tasksLoading, taskError, compatibilityError, unavailableTargetError, parsedRepetitions.error, parsedRepetitions.value, parsedWarmupRuns.error, parsedWarmupRuns.value, parsedConcurrency.error, parsedConcurrency.value]);

  useEffect(() => {
    let mounted = true;
    listRunJobs()
      .then(nextJobs => {
        if (!mounted) {
          return;
        }
        setJobs(nextJobs);
        const running = nextJobs.find(isJobActive);
        if (running) {
          setActiveJobId(running.id);
        }
      })
      .catch(error => setMessage(String(error)));
    return () => {
      mounted = false;
    };
  }, [setMessage]);

  useEffect(() => {
    if (!activeJobId) {
      return undefined;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    async function poll() {
      try {
        const job = await getRunJob(activeJobId);
        if (cancelled) {
          return;
        }
        if (job) {
          setJobs(current => mergeJob(current, job));
          if (!isJobActive(job)) {
            await handleFinishedRunJob(job);
            return;
          }
        }
      } catch (error) {
        if (!cancelled) {
          setMessage(String(error));
        }
      }
      timer = setTimeout(poll, 1000);
    }

    poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [activeJobId, refresh, setMessage, setPage]);

  async function handleFinishedRunJob(job: RunJob) {
    setActiveJobId('');
    setJobs(current => mergeJob(current, job));
    await refresh();
    const resultNote = job.results.length ? ` (${job.results.length} result(s) available)` : '';
    setMessage(job.status === 'completed'
      ? `${job.completed}/${job.total} benchmark task runs completed`
      : `${job.message}${resultNote}`);
    if (job.results.length || job.status === 'completed') {
      setPage('results');
    }
  }

  async function startRun() {
    if (!selected.length) {
      setMessage('Select at least one target');
      return;
    }
    if (!selectedTaskIds.length) {
      setMessage(taskSelectionError || 'Select at least one benchmark task');
      return;
    }
    if (parsedRepetitions.error || parsedRepetitions.value == null || parsedWarmupRuns.error || parsedWarmupRuns.value == null || parsedConcurrency.error || parsedConcurrency.value == null || parsedMaxCostUsd.error) {
      setMessage(parsedRepetitions.error || parsedWarmupRuns.error || parsedConcurrency.error || parsedMaxCostUsd.error || 'Run settings are invalid');
      return;
    }
    if (costLimitMessage) {
      if (runEstimate?.unpricedTargets.length) {
        repairRunPricingBlockers(runEstimate.unpricedTargets);
      } else {
        setRunAdvancedOpen(true);
        setMessage(`${costLimitMessage}. Adjust Max cost USD, reduce targets/tasks/repetitions, or clear the cap for an intentional uncapped run.`);
      }
      return;
    }
    setBusy(true);
    try {
      setRunValidationBlockers([]);
      setMessage(`Validating ${selected.length} selected target(s) before run`);
      const validationResults = await Promise.all(selected.map(id => validateTarget(id)));
      await refresh();
      const blockers = validationResults.filter(result => result.status === 'error');
      if (blockers.length) {
        setRunValidationBlockers(blockers);
        setMessage(`Run blocked: ${formatValidationCodeCounts(blockers)}. Fix validation errors or choose different targets.`);
        return;
      }
      const warnings = validationResults.filter(result => result.status !== 'ok');
      const warningNote = warnings.length ? `; warnings: ${formatValidationCodeCounts(warnings)}` : '';
      const costNote = parsedMaxCostUsd.value == null ? '' : `, max cost ${formatCost(parsedMaxCostUsd.value)}`;
      setMessage(`Starting ${selectedPackId} job with ${selectedTaskIds.length} task(s), ${parsedRepetitions.value} repetition(s), ${parsedWarmupRuns.value} warmup(s), concurrency ${parsedConcurrency.value}${costNote}${warningNote}`);
      const job = await startRunJob(selected, docker, selectedPackId, parsedRepetitions.value, parsedWarmupRuns.value, parsedConcurrency.value, parsedMaxCostUsd.value, selectedTaskIds);
      setJobs(current => mergeJob(current, job));
      if (isJobActive(job)) {
        setActiveJobId(job.id);
        setMessage(`Started run job ${job.id.slice(0, 8)}`);
      } else {
        await handleFinishedRunJob(job);
      }
    } catch (error) {
      setMessage(benchmarkRunFailureMessage(error));
    } finally {
      setBusy(false);
    }
  }
  function repairRunValidationBlocker(blocker: TargetValidation) {
    const code = validationRepairCode(blocker);
    openTargetRepair({ targetIds: [blocker.targetId], code });
    setMessage(`${errorCategoryRepairHint(code)} for ${blocker.targetId}`);
  }
  function repairRunPricingBlockers(targetIds: string[]) {
    if (!targetIds.length) {
      setMessage('No unpriced selected target found to repair');
      return;
    }
    openTargetRepair({ targetIds, code: 'pricing_assumption' });
    setMessage(`Add input/output pricing before running a capped local/cloud comparison: ${previewList(targetIds)}`);
  }
  function openLocalSetupFromRunBuilder() {
    openTargetSetup({ code: 'local_runtime_detect', benchmarkPackId: setupBenchmarkPackId, targetIds: selectedCloudTargetIds });
    setMessage(`Detecting local runtimes for this ${benchmarkPackLabel(setupBenchmarkPackId, modelBenchmarkPacks)} local/cloud comparison`);
  }
  function openHfLocalSetupFromRunBuilder() {
    openHuggingFaceLocalSetup({ benchmarkPackId: setupBenchmarkPackId, targetIds: selectedCloudTargetIds });
    setMessage(huggingFaceLocalModelSetupMessage(targets, selectedCloudTargetIds, setupBenchmarkPackId));
  }
  function openCloudSetupFromRunBuilder() {
    openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId: setupBenchmarkPackId, targetIds: selectedLocalTargetIds });
    setMessage(`Preparing cloud target setup for this ${benchmarkPackLabel(setupBenchmarkPackId, modelBenchmarkPacks)} local/cloud comparison`);
  }
  async function cancelRun(id: string) {
    setMessage(`Cancelling run job ${id.slice(0, 8)}`);
    try {
      const job = await cancelRunJob(id);
      if (job) {
        setJobs(current => mergeJob(current, job));
        if (isJobActive(job)) {
          setActiveJobId(job.id);
        } else {
          await handleFinishedRunJob(job);
        }
      }
    } catch (error) {
      setMessage(String(error));
    }
  }
  async function replayRun(id: string, mode: 'duplicate' | 'retry') {
    const sourceJob = jobs.find(job => job.id === id);
    setBusy(true);
    setMessage(`${mode === 'retry' ? 'Retrying' : 'Duplicating'} run job ${id.slice(0, 8)}`);
    try {
      const job = mode === 'retry' ? await retryRunJob(id) : await duplicateRunJob(id);
      if (job) {
        setJobs(current => mergeJob(current, job));
        if (isJobActive(job)) {
          setActiveJobId(job.id);
          setMessage(`${mode === 'retry' ? retryJobScopeLabel(sourceJob, job) : 'Duplicated'} run job ${job.id.slice(0, 8)} queued`);
        } else {
          await handleFinishedRunJob(job);
        }
      }
    } catch (error) {
      setMessage(benchmarkRunFailureMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function clearFinished() {
    setBusy(true);
    try {
      const count = await clearFinishedRunJobs();
      setJobs(current => current.filter(isJobActive));
      setMessage(`Cleared ${count} finished run jobs`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  async function openJobResults(job: RunJob) {
    setBusy(true);
    try {
      await refresh();
      openResultsForGroup(job.runGroupId, job.results[0]?.id);
      setMessage(`Opened Results for run group ${job.runGroupId.slice(0, 8)}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  async function exportJobReport(job: RunJob) {
    const runIds = job.results.map(result => result.id);
    if (!runIds.length) {
      setMessage(`Run job ${job.id.slice(0, 8)} has no stored results to export`);
      return;
    }
    setBusy(true);
    try {
      const path = await exportReportFolder(runIds);
      await navigator.clipboard.writeText(path);
      setMessage(`Report folder created for run job ${job.id.slice(0, 8)} (${runIds.length} result(s)): ${path}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  function applyTargetShortcut(label: string, targetIds: string[], benchmarkPackId?: string) {
    if (!targetIds.length) {
      setMessage(`No ${label} targets available`);
      return;
    }
    setSelected(targetIds);
    const nextPackId = benchmarkPackId && packs.some(pack => pack.id === benchmarkPackId)
      ? benchmarkPackId
      : preferredRunPackForTargets(targetIds, targets, packs);
    if (nextPackId) {
      setSelectedPackId(nextPackId);
    }
    const packNote = nextPackId
      ? ` using ${packs.find(pack => pack.id === nextPackId)?.name ?? benchmarkPackLabel(nextPackId)}`
      : '';
    const includesCloudTarget = targetIds.some(id => cloudTargetIds.includes(id));
    if (includesCloudTarget) {
      setMaxCostUsd(String(defaultComparisonMaxCostUsd));
    }
    if (label.startsWith('local/cloud')) {
      setRepetitions('3');
      setWarmupRuns('1');
      setConcurrency(String(Math.min(2, Math.max(1, targetIds.length))));
      const selectedUnpricedCloudTargetIds = targetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId));
      const costCapNote = includesCloudTarget ? ` and ${formatCost(defaultComparisonMaxCostUsd)} max-cost cap` : '';
      const pricingNote = selectedUnpricedCloudTargetIds.length
        ? ` Add pricing for ${previewList(targetLabelsById(selectedUnpricedCloudTargetIds, targets))} before running with the cap.`
        : skippedUnpricedCloudTargetIds.length
        ? ` Skipped unpriced cloud target(s): ${previewList(targetLabelsById(skippedUnpricedCloudTargetIds, targets))}.`
        : pricedCloudTargets.length
          ? ' Selected priced cloud target(s) for cost-capped comparison.'
          : includesCloudTarget
            ? ' Add cloud pricing before running with a max-cost cap.'
            : '';
      setMessage(`Selected ${targetIds.length} ${label} target(s)${packNote} with 3 repetitions, 1 warmup${costCapNote}.${pricingNote}`);
      return;
    }
    const costCapNote = includesCloudTarget ? ` with ${formatCost(defaultComparisonMaxCostUsd)} max-cost cap` : '';
    const selectedUnpricedCloudTargetIds = targetIds.filter(targetId => unpricedCloudTargetIds.includes(targetId));
    const pricingNote = selectedUnpricedCloudTargetIds.length
      ? `. Add pricing for ${previewList(targetLabelsById(selectedUnpricedCloudTargetIds, targets))} before running with the cap`
      : '';
    setMessage(`Selected ${targetIds.length} ${label} target(s)${packNote}${costCapNote}${pricingNote}`);
  }
  function applyRecommendedCostCap(value: number) {
    setMaxCostUsd(String(value));
    setMessage(`Set max-cost cap to ${formatCost(value)} for selected cloud target(s)`);
  }
  function applyEstimatedRunCostCap(value: number) {
    setMaxCostUsd(String(value));
    setMessage(`Raised max-cost cap to ${formatCost(value)} for this estimated run`);
  }
  function applyRecommendedComparisonDefaults() {
    setRepetitions(String(recommendedTaskRepetitions));
    setWarmupRuns('1');
    setConcurrency(String(recommendedComparisonConcurrency));
    if (selectedCloudTargets.length && !maxCostUsd) {
      setMaxCostUsd(String(defaultComparisonMaxCostUsd));
    }
    const capNote = selectedCloudTargets.length && !maxCostUsd ? ` and ${formatCost(defaultComparisonMaxCostUsd)} cap` : '';
    setMessage(`Applied comparison defaults: ${recommendedTaskRepetitions} repetitions, 1 warmup, concurrency ${recommendedComparisonConcurrency}${capNote}`);
  }
  function setTaskSelected(taskId: string, checked: boolean) {
    setSelectedTaskIds(current => {
      const next = new Set(current);
      if (checked) {
        next.add(taskId);
      } else {
        next.delete(taskId);
      }
      return packTasks.map(task => task.id).filter(id => next.has(id));
    });
  }
  function selectAllTasks() {
    setSelectedTaskIds(packTasks.map(task => task.id));
  }
  function clearTaskSelection() {
    setSelectedTaskIds([]);
  }
  const hasReliabilityPack = packs.some(pack => pack.id === 'llm-reliability');
  const runPackDisabled = busy || !selected.length || activeRunInProgress || !selectedPackId || Boolean(runSettingsError);
  const runPackTitle = activeRunInProgress
    ? 'A benchmark job is already running'
    : busy
      ? 'BenchForge is busy'
      : !selected.length
        ? 'Select at least one target before running a benchmark pack'
        : !selectedPackId
          ? 'Choose a benchmark pack before starting'
          : runSettingsError
            ? runSettingsError
            : `Validate and run ${selected.length} target(s) on ${selectedTaskCount} selected task(s)`;
  const clearFinishedDisabled = busy || !jobs.some(isJobFinished);
  const clearFinishedTitle = busy
    ? 'BenchForge is busy'
    : jobs.some(isJobFinished)
      ? 'Clear completed, failed, or cancelled run jobs from this table'
      : 'No finished run jobs to clear';
  return <section><div className="section-head"><h1>Run Builder</h1><div className="actions"><button disabled={runPackDisabled} title={runPackTitle} onClick={startRun}><Play size={16} />Run pack</button><button disabled={clearFinishedDisabled} title={clearFinishedTitle} onClick={() => clearFinished().catch(error => setMessage(String(error)))}><Trash2 size={16} />Clear finished</button></div></div>
    <div className="panel compact">
      <div className="form-grid">
        <label>Benchmark pack <select value={selectedPackId} onChange={event => setSelectedPackId(event.target.value)}>{packs.map(pack => <option key={pack.id} value={pack.id}>{pack.name} ({pack.tasks})</option>)}</select></label>
        <label>Repetitions <input type="number" min="1" max="100" step="1" value={repetitions} onChange={event => setRepetitions(event.target.value)} /></label>
        <details className="advanced-section" open={runAdvancedOpen || runAdvancedHasError} onToggle={event => setRunAdvancedOpen(event.currentTarget.open)}>
          <summary><SlidersHorizontal size={14} />Advanced run settings</summary>
          <div className="form-grid">
            <label>Warmup runs <input type="number" min="0" max="20" step="1" value={warmupRuns} onChange={event => setWarmupRuns(event.target.value)} /></label>
            <label>Concurrency <input type="number" min="1" max="8" step="1" value={concurrency} onChange={event => setConcurrency(event.target.value)} /></label>
            <label>Max cost USD <input type="number" min="0" step="0.000001" value={maxCostUsd} onChange={event => setMaxCostUsd(event.target.value)} /></label>
            <label className="toggle"><input type="checkbox" checked={docker} onChange={event => setDocker(event.target.checked)} />Docker scoring (network off)</label>
          </div>
        </details>
      </div>
      <details className="advanced-section task-selection-section" open={taskSelectionOpen || taskSelectionNeedsAttention} onToggle={event => setTaskSelectionOpen(event.currentTarget.open)}>
        <summary><ClipboardCheck size={14} />Tasks <span className="mini-tag">{selectedTaskCount}/{packTasks.length || selectedPack?.tasks || 0} selected</span></summary>
        <div className="section-inner">
          <div className="subsection-head"><strong>Task selection</strong><div className="actions"><button disabled={tasksLoading || !packTasks.length || selectedTaskCount === packTasks.length} onClick={selectAllTasks}><ClipboardCheck size={14} />All</button><button disabled={tasksLoading || !selectedTaskCount} onClick={clearTaskSelection}><Square size={14} />None</button></div></div>
          {tasksLoading ? <p className="muted">Loading benchmark tasks...</p> : taskError ? <p className="muted">{taskError}</p> : <div className="checks task-checks">{packTasks.map(task => {
            const checked = selectedTaskIds.includes(task.id);
            return <label key={task.id} title={task.prompt}>
              <input type="checkbox" checked={checked} onChange={event => setTaskSelected(task.id, event.target.checked)} />
              <span>{task.name}</span>
              <span className="mini-tag">{task.taskType}</span>
              <span className="mini-tag">{task.timeoutSeconds}s</span>
              {task.scoringMethods.slice(0, 2).map(method => <span className="mini-tag" key={`${task.id}-${method}`}>{method}</span>)}
            </label>;
          })}</div>}
        </div>
      </details>
      {selectedPack?.heavy ? <p className="muted">Selected pack is marked heavy and may take longer or require external tooling.</p> : null}
      {selectedPack ? <PackRunReadiness pack={selectedPack} docker={docker} /> : null}
      {comparisonRunReadiness ? <LocalCloudRunReadinessPanel
        readiness={comparisonRunReadiness}
        onUseLocalCloud={canApplyLocalCloudPanelShortcut ? () => applyTargetShortcut('local/cloud comparison', localCloudTargetIds, localCloudPanelShortcutPackId) : undefined}
        onUseAllComparable={canApplyAllComparablePanelShortcut ? () => applyTargetShortcut('local/cloud comparison', localCloudTargetIds, localCloudPanelShortcutPackId) : undefined}
        localCloudActionLabel={localCloudReadinessShortcutLabel}
        allComparableActionLabel={allComparableReadinessShortcutLabel}
        onAddLocalTarget={canOpenLocalSetupFromReadiness ? openLocalSetupFromRunBuilder : undefined}
        onAddHfLocalModel={canOpenHfLocalSetupFromReadiness ? openHfLocalSetupFromRunBuilder : undefined}
        onAddCloudTarget={canOpenCloudSetupFromReadiness ? openCloudSetupFromRunBuilder : undefined}
        onSetCostCap={applyRecommendedCostCap}
        onRepairPricing={repairRunPricingBlockers}
      /> : null}
      {runValidationBlockers.length ? <RunValidationBlockerPanel blockers={runValidationBlockers} targets={targets} onRepair={repairRunValidationBlocker} /> : null}
      {repetitionConfidenceWarning ? <div className="preflight-box warn"><strong>Repetition Confidence</strong><p>{repetitionConfidenceWarning}</p>{comparisonDefaultsRecommended ? <div className="row-actions"><button onClick={applyRecommendedComparisonDefaults}><ShieldCheck size={14} />Use 3 reps + warmup</button></div> : null}</div> : null}
      {runScaleWarning ? <div className="preflight-box warn"><strong>Large Run</strong><p>{runScaleWarning}</p></div> : null}
      {runSettingsError ? <RunSettingsBlockerPanel
        message={runSettingsError}
        pricingRepairTargetIds={runCostPricingRepairTargetIds}
        recommendedCostCapUsd={recommendedRunCostCapUsd}
        canAdjustCostCap={canAdjustRunCostCap}
        onRepairPricing={repairRunPricingBlockers}
        onUseCostCap={applyEstimatedRunCostCap}
        onAdjustCostCap={() => setRunAdvancedOpen(true)}
      /> : <RunEstimatePanel estimate={runEstimate} error={estimateError} fallbackRuns={estimatedRuns} fallbackWarmups={estimatedWarmups} selectedTargets={selected.length} packTasks={selectedTaskCount} repetitions={parsedRepetitions.value ?? 1} concurrency={parsedConcurrency.value ?? 1} maxCostUsd={parsedMaxCostUsd.value} />}
      <div className="row-actions target-shortcuts">
        <button disabled={!modelTargetIds.length} title={allModelsShortcutTitle} onClick={() => applyTargetShortcut('model', modelTargetIds)}><Boxes size={14} />All models</button>
        <button disabled={!localTargetIds.length} title={localTargetIds.length ? 'Select all enabled local model targets' : 'Add one enabled local model target'} onClick={() => applyTargetShortcut('local', localTargetIds)}><TerminalSquare size={14} />Local</button>
        <button disabled={!cloudTargetIds.length} title={cloudShortcutTitle} onClick={() => applyTargetShortcut('cloud', cloudTargetIds)}><Database size={14} />Cloud</button>
        <button disabled={!localCloudTargetIds.length} title={localCloudShortcutTitle} onClick={() => applyTargetShortcut('local/cloud comparison', localCloudTargetIds)}><ClipboardCheck size={14} />{localCloudShortcutLabel}</button>
        <button disabled={!localCloudTargetIds.length || !hasReliabilityPack} title={hasReliabilityPack ? localCloudReliabilityShortcutTitle : 'Reliability benchmark pack is not available'} onClick={() => applyTargetShortcut('local/cloud reliability', localCloudTargetIds, 'llm-reliability')}><FlaskConical size={14} />Reliability</button>
        {localCloudTargetIds.length ? <span className={`mini-tag ${localCloudSelectedUnpricedCloudTargetIds.length ? 'warn' : ''}`} title={localCloudShortcutTitle}>
          {localCloudSelectedUnpricedCloudTargetIds.length ? 'pricing needed' : 'cost-ready'}
        </span> : null}
        {localCloudSelectedUnpricedCloudTargetIds.length ? <button title={localCloudShortcutTitle} onClick={() => repairRunPricingBlockers(localCloudSelectedUnpricedCloudTargetIds)}><Pencil size={14} />Add pricing</button> : null}
      </div>
      <div className="checks target-checks">{targets.map(target => {
        const checked = selected.includes(target.id);
        const compatible = targetCompatibleWithPack(target, selectedPack);
        const selectable = targetIsSelectableForRun(target);
        const localModelTarget = isLocalModelTarget(target);
        const cloudModelTarget = isCloudModelTarget(target);
        const cloudPricingConfigured = cloudModelTarget && targetHasInputOutputPricing(target);
        const disabled = (!compatible && !checked) || (!selectable && !checked);
        const targetHint = !selectable
          ? 'Target is invalid or disabled; validate or edit it before adding it to a run.'
          : compatible
            ? runBuilderTargetHint(target, localModelTarget, cloudModelTarget, cloudPricingConfigured)
            : `Not supported by ${selectedPack?.name ?? 'selected pack'}`;
        return <label key={target.id} className={compatible && selectable ? '' : 'incompatible-target'} title={targetHint}>
          <input type="checkbox" disabled={disabled} checked={checked} onChange={event => setSelected(event.target.checked ? [...selected, target.id] : selected.filter(id => id !== target.id))} />
          <span>{target.name}</span>
          <span className="mini-tag">{target.kind}</span>
          {localModelTarget ? <span className="mini-tag">local</span> : null}
          {cloudModelTarget ? <span className="mini-tag">cloud</span> : null}
          {cloudModelTarget ? <span className={`mini-tag ${cloudPricingConfigured ? '' : 'warn'}`}>{cloudPricingConfigured ? 'priced' : 'needs pricing'}</span> : null}
          {!selectable ? <span className="mini-tag warn">not runnable</span> : null}
          {!compatible ? <span className="mini-tag warn">not for pack</span> : null}
        </label>;
      })}</div>
    </div>
    <h2>Run Jobs</h2>
    <table><thead><tr><th>Job</th><th>Group</th><th>Pack</th><th>Plan</th><th>Status</th><th>Progress</th><th>Message</th><th>Results</th><th></th></tr></thead><tbody>{jobs.length ? jobs.map(job => {
      const percent = job.total ? Math.round((job.completed / job.total) * 100) : 0;
      return <tr key={job.id}><td>{job.id.slice(0, 8)}</td><td>{job.runGroupId.slice(0, 8)}</td><td>{job.benchmarkPackId}</td><td>{jobPlanSummary(job)}</td><td><span className={`pill ${jobStatusClass(job.status)}`}>{job.status}</span></td><td><div className="progress-cell"><div className="progress-track"><div className="progress-fill" style={{ width: `${Math.min(percent, 100)}%` }} /></div><span>{job.completed}/{job.total || '-'}</span></div></td><td><JobMessageCell job={job} /></td><td>{job.results.length}</td><td><div className="row-actions">{job.results.length ? <><button disabled={busy} title="Open Results for this run group" onClick={() => openJobResults(job)}><ClipboardCheck size={14} />Results</button><button disabled={busy} title="Export a report folder for this job's stored results" onClick={() => exportJobReport(job)}><Download size={14} />Report</button></> : null}{isJobActive(job) ? <button disabled={job.status === 'cancelling'} title="Cancel run job" onClick={() => cancelRun(job.id).catch(error => setMessage(String(error)))}><Square size={14} />Stop</button> : <><button disabled={busy} title="Duplicate run job" onClick={() => replayRun(job.id, 'duplicate').catch(error => setMessage(String(error)))}><Copy size={14} />Duplicate</button>{isJobRetryable(job) ? <button disabled={busy} title="Retry failed job" onClick={() => replayRun(job.id, 'retry').catch(error => setMessage(String(error)))}><RotateCcw size={14} />Retry</button> : null}</>}</div></td></tr>;
    }) : <tr><td colSpan={9} className="muted">No run jobs found.</td></tr>}</tbody></table>
  </section>;
}

function PackRunReadiness({ pack, docker }: { pack: BenchmarkPack; docker: boolean }) {
  const hasDockerEligibleTasks = pack.languages.includes('python') && !pack.taskTypes.every(type => type === 'prompt');
  const notes = [
    pack.description,
    `Fit: ${pack.targetFit}`,
    `Evidence: ${formatEvidenceProfile(pack.evidenceProfile)}; ${pack.promptTasks} prompt task(s), total task weight ${formatNumber(pack.totalTaskWeight)}.`,
    packCalibrationSummary(pack),
    pack.calibrationNotes,
    ...pack.evidenceWarnings,
    pack.estimatedRuntime ? `Estimated runtime: ${pack.estimatedRuntime}` : null,
    pack.requiresSandbox && !docker ? 'Sandbox required by pack metadata; enable Docker scoring or use a safe local setup.' : null,
    docker && hasDockerEligibleTasks ? 'Docker scoring preflights Docker or Colima and runs eligible Python scoring tasks with Docker networking disabled; other tasks use sanitized host scoring.' : null,
    docker && !hasDockerEligibleTasks ? 'This pack has no Docker-eligible Python scoring tasks; scoring uses the standard local path.' : null,
    pack.requiredTools.length ? `Required tools: ${formatList(pack.requiredTools)}` : null,
    pack.scoringMethods.length ? `Scoring: ${formatList(pack.scoringMethods)}` : null,
  ].filter(Boolean);
  const hasEvidenceWarning = pack.evidenceWarnings.length > 0;
  return <div className={`preflight-box ${pack.requiresSandbox && !docker || hasEvidenceWarning ? 'warn' : ''}`}>
    <strong>{pack.name}</strong>
    {notes.map(note => <p key={note} className="muted">{note}</p>)}
    <div className="tag-row">
      {pack.tags.map(tag => <span className="mini-tag" key={`${pack.id}-run-${tag}`}>{tag}</span>)}
      {pack.taskTypes.map(type => <span className="mini-tag" key={`${pack.id}-type-${type}`}>{type}</span>)}
      {pack.supportedTargetKinds.map(kind => <span className="mini-tag" key={`${pack.id}-kind-${kind}`}>{kind}</span>)}
    </div>
  </div>;
}

function LocalCloudRunReadinessPanel({
  readiness,
  onUseLocalCloud,
  onUseAllComparable,
  localCloudActionLabel = 'Use local + cloud',
  allComparableActionLabel = 'Use all comparable',
  onAddLocalTarget,
  onAddHfLocalModel,
  onAddCloudTarget,
  onSetCostCap,
  onRepairPricing,
}: {
  readiness: LocalCloudRunReadiness;
  onUseLocalCloud?: () => void;
  onUseAllComparable?: () => void;
  localCloudActionLabel?: string;
  allComparableActionLabel?: string;
  onAddLocalTarget?: () => void;
  onAddHfLocalModel?: () => void;
  onAddCloudTarget?: () => void;
  onSetCostCap?: (value: number) => void;
  onRepairPricing?: (targetIds: string[]) => void;
}) {
  const hasActions = Boolean(onUseLocalCloud || onUseAllComparable || onAddLocalTarget || onAddHfLocalModel || onAddCloudTarget || (readiness.recommendedCostCapUsd != null && onSetCostCap) || (readiness.pricingRepairTargetIds.length && onRepairPricing));
  return <div className={`preflight-box ${readiness.tone}`}>
    <strong>Comparison Readiness</strong>
    <p>{readiness.headline}</p>
    <div className="mini-grid">{readiness.facts.map(fact => <span key={fact}>{fact}</span>)}</div>
    {readiness.notes.map(note => <p key={note}>{note}</p>)}
    {hasActions ? <div className="row-actions">
      {onUseLocalCloud ? <button onClick={onUseLocalCloud}><ClipboardCheck size={14} />{localCloudActionLabel}</button> : null}
      {onUseAllComparable ? <button onClick={onUseAllComparable}><Boxes size={14} />{allComparableActionLabel}</button> : null}
      {onAddLocalTarget ? <button onClick={onAddLocalTarget}><Search size={14} />Detect runtime</button> : null}
      {onAddHfLocalModel ? <button onClick={onAddHfLocalModel}><Download size={14} />HF model</button> : null}
      {onAddCloudTarget ? <button onClick={onAddCloudTarget}><Boxes size={14} />Add cloud target</button> : null}
      {readiness.recommendedCostCapUsd != null && onSetCostCap ? <button onClick={() => onSetCostCap(readiness.recommendedCostCapUsd!)}><ShieldCheck size={14} />Set {formatCost(readiness.recommendedCostCapUsd)} cap</button> : null}
      {readiness.pricingRepairTargetIds.length && onRepairPricing ? <button title={errorCategoryRepairHint('pricing_assumption')} onClick={() => onRepairPricing(readiness.pricingRepairTargetIds)}><Pencil size={14} />Add pricing</button> : null}
    </div> : null}
  </div>;
}

function runBuilderComparisonShortcutLabel(targetIds: string[], fallback: string) {
  return targetIds.length >= 2 ? `Compare ${targetIds.length} models` : fallback;
}

function runBuilderUseComparisonShortcutLabel(targetIds: string[]) {
  return targetIds.length >= 2 ? `Use ${targetIds.length}-model comparison` : 'Use local + cloud';
}

function runBuilderUseAllComparisonShortcutLabel(targetIds: string[]) {
  return targetIds.length >= 2 ? `Use all ${targetIds.length} models` : 'Use all comparable';
}

function targetShortcutHelp(label: string, targetIds: string[], unpricedCloudTargetIds: string[], targets: Target[]) {
  if (!targetIds.length) {
    return `Add enabled ${label} before using this shortcut`;
  }
  const targetById = new Map(targets.map(target => [target.id, target]));
  const includesCloudTarget = targetIds.some(targetId => {
    const target = targetById.get(targetId);
    return Boolean(target && isCloudModelTarget(target));
  });
  if (unpricedCloudTargetIds.length) {
    return `Select ${label} and set a max-cost cap. Add input/output pricing for ${previewList(targetLabelsById(unpricedCloudTargetIds, targets))} before running with the cap.`;
  }
  return includesCloudTarget
    ? `Select ${label} and set a ${formatCost(defaultComparisonMaxCostUsd)} max-cost cap`
    : `Select ${label}`;
}

function runBuilderTargetHint(target: Target, localModelTarget: boolean, cloudModelTarget: boolean, cloudPricingConfigured: boolean) {
  if (localModelTarget) {
    return `${target.kind} local model target`;
  }
  if (cloudModelTarget) {
    return cloudPricingConfigured
      ? `${target.kind} cloud model target with input/output pricing for capped cost estimates`
      : `${target.kind} cloud model target. Add input/output pricing before capped local/cloud comparisons.`;
  }
  return `${target.kind} target`;
}

function TargetModelTags({ target }: { target: Target }) {
  const localModelTarget = isLocalModelTarget(target);
  const cloudModelTarget = isCloudModelTarget(target);
  if (!localModelTarget && !cloudModelTarget) {
    return null;
  }
  const cloudPricingConfigured = cloudModelTarget && targetHasInputOutputPricing(target);
  return <div className="tag-row">
    {localModelTarget ? <span className="mini-tag">local</span> : null}
    {cloudModelTarget ? <span className="mini-tag">cloud</span> : null}
    {cloudModelTarget ? <span className={`mini-tag ${cloudPricingConfigured ? '' : 'warn'}`}>{cloudPricingConfigured ? 'priced' : 'needs pricing'}</span> : null}
  </div>;
}

function localCloudShortcutHelp(
  targetIds: string[],
  selectedUnpricedCloudTargetIds: string[],
  skippedUnpricedCloudTargetIds: string[],
  targets: Target[],
  packName = 'model comparison',
) {
  if (!targetIds.length) {
    return 'Add one enabled local model target and one enabled cloud model target';
  }
  const scope = `${targetIds.length} comparable local/priced cloud target(s)`;
  if (selectedUnpricedCloudTargetIds.length) {
    return `Select ${scope} for ${packName}. Add input/output pricing for ${previewList(targetLabelsById(selectedUnpricedCloudTargetIds, targets))} before running with the max-cost cap.`;
  }
  if (skippedUnpricedCloudTargetIds.length) {
    return `Select ${scope} for ${packName}; skips unpriced cloud target(s): ${previewList(targetLabelsById(skippedUnpricedCloudTargetIds, targets))}.`;
  }
  return `Select ${scope} for ${packName} with a ${formatCost(defaultComparisonMaxCostUsd)} max-cost cap`;
}

function RunValidationBlockerPanel({ blockers, targets, onRepair }: { blockers: TargetValidation[]; targets: Target[]; onRepair: (blocker: TargetValidation) => void }) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  return <div className="preflight-box error">
    <strong>Run Blocked</strong>
    <p>Fix target validation errors before starting this benchmark.</p>
    <table className="compact-table run-blocker-table"><thead><tr><th>Target</th><th>Error</th><th>Detail</th><th></th></tr></thead><tbody>
      {blockers.map(blocker => {
        const target = targetById.get(blocker.targetId);
        const code = validationRepairCode(blocker);
        const editable = target ? targetRepairTargetCanEdit(target) : false;
        return <tr key={blocker.targetId}>
          <td><strong>{target?.name ?? blocker.targetId}</strong><div className="muted">{blocker.targetId}</div></td>
          <td>{code}</td>
          <td className="diagnostic-detail">{blocker.detail}</td>
          <td><button disabled={!editable} title={editable ? errorCategoryRepairHint(code) : 'Target is missing, disabled, or not editable from Targets'} onClick={() => onRepair(blocker)}><Wrench size={14} />Repair</button></td>
        </tr>;
      })}
    </tbody></table>
  </div>;
}

function RunSettingsBlockerPanel({
  message,
  pricingRepairTargetIds,
  recommendedCostCapUsd,
  canAdjustCostCap,
  onRepairPricing,
  onUseCostCap,
  onAdjustCostCap,
}: {
  message: string;
  pricingRepairTargetIds: string[];
  recommendedCostCapUsd?: number;
  canAdjustCostCap: boolean;
  onRepairPricing: (targetIds: string[]) => void;
  onUseCostCap: (value: number) => void;
  onAdjustCostCap: () => void;
}) {
  const hasActions = Boolean(pricingRepairTargetIds.length || recommendedCostCapUsd != null || canAdjustCostCap);
  return <div className="preflight-box warn">
    <strong>Run Blocked</strong>
    <p>{message}</p>
    {hasActions ? <div className="row-actions">
      {pricingRepairTargetIds.length ? <button title={errorCategoryRepairHint('pricing_assumption')} onClick={() => onRepairPricing(pricingRepairTargetIds)}><Pencil size={14} />Add pricing</button> : null}
      {recommendedCostCapUsd != null ? <button onClick={() => onUseCostCap(recommendedCostCapUsd)}><ShieldCheck size={14} />Use {formatCost(recommendedCostCapUsd)} cap</button> : null}
      {canAdjustCostCap ? <button onClick={onAdjustCostCap}><SlidersHorizontal size={14} />Adjust cap</button> : null}
    </div> : null}
  </div>;
}

function localCloudRunReadiness({
  pack,
  selectedLocalCount,
  selectedCloudCount,
  availableLocalCount,
  availableCloudCount,
  selectedTaskCount,
  totalTaskCount,
  repetitions,
  warmupRuns,
  concurrency,
  maxCostUsd,
  estimate,
}: {
  pack: BenchmarkPack;
  selectedLocalCount: number;
  selectedCloudCount: number;
  availableLocalCount: number;
  availableCloudCount: number;
  selectedTaskCount: number;
  totalTaskCount: number;
  repetitions?: number;
  warmupRuns?: number;
  concurrency?: number;
  maxCostUsd?: number;
  estimate: RunEstimate | null;
}): LocalCloudRunReadiness {
  const hasLocal = selectedLocalCount > 0;
  const hasCloud = selectedCloudCount > 0;
  const hasBoth = hasLocal && hasCloud;
  const recommendedCostCapUsd = hasCloud && maxCostUsd == null ? defaultComparisonMaxCostUsd : undefined;
  const pricingCoverageKnown = Boolean(estimate);
  const pricingRepairTargetIds = estimate?.unpricedTargets ?? [];
  const missingPricingCount = pricingRepairTargetIds.length;
  const minComparisonTasks = 3;
  const enoughSelectedTasks = selectedTaskCount >= minComparisonTasks;
  const isPromptPack = pack.taskTypes.includes('prompt');
  const isConnectivitySmoke = pack.evidenceProfile === 'connectivity_smoke' || pack.id === 'llm-connectivity';
  const promptPackEvidenceReady = !isPromptPack || pack.evidenceProfile === 'prompt_comparison';
  const notes: string[] = [];

  if (hasBoth) {
    notes.push(`Selected ${selectedLocalCount} local and ${selectedCloudCount} cloud model target(s), so this run can produce side-by-side deployment evidence.`);
  } else if (hasLocal && availableCloudCount > 0) {
    notes.push('Only local model targets are selected. Add a cloud target or use the local/cloud shortcut to make the run comparable.');
  } else if (hasCloud && availableLocalCount > 0) {
    notes.push('Only cloud model targets are selected. Add a local target or use the local/cloud shortcut to make the run comparable.');
  } else {
    notes.push('Add at least one local model target and one cloud model target before treating results as a local/cloud comparison.');
  }

  if (isConnectivitySmoke) {
    notes.push('LLM Connectivity is an endpoint sanity check. Use Reliability, Structured Output, Grounded Context, Core, Practical Selection, or Decision Suite when choosing a model.');
  } else if (isPromptPack) {
    notes.push(`${pack.name} has ${totalTaskCount} prompt task(s); ${selectedTaskCount} are selected for this run.`);
    pack.evidenceWarnings.forEach(warning => notes.push(warning));
    if (pack.calibrationStatus !== 'calibrated') {
      notes.push(`${packCalibrationSummary(pack)}. Treat this as first-pass comparison evidence, not definitive benchmark calibration.`);
    }
    if (!enoughSelectedTasks) {
      notes.push(`Select at least ${minComparisonTasks} prompt tasks before treating this as broad model-selection evidence. A smaller subset is useful for diagnosis or quick checks.`);
    } else if (selectedTaskCount < totalTaskCount) {
      notes.push('This task subset can compare the selected behavior, but the full pack gives broader evidence across failure modes.');
    } else if (!promptPackEvidenceReady) {
      notes.push('Improve the pack evidence profile before treating this prompt suite as model-selection evidence.');
    } else {
      notes.push('Running the full prompt pack gives broader evidence than a connectivity check when repetitions are high enough.');
    }
  }

  if (repetitions == null || repetitions < recommendedTaskRepetitions) {
    notes.push(`Use at least ${recommendedTaskRepetitions} repetitions per task/target for model-selection confidence.`);
  }
  if (warmupRuns == null || warmupRuns < 1) {
    notes.push('Add at least one warmup run when comparing latency so first-call startup costs do not dominate the result.');
  }
  if (recommendedCostCapUsd != null) {
    notes.push(`Set a max-cost cap before running paid cloud targets for stronger spend control. ${formatCost(recommendedCostCapUsd)} is the default comparison cap.`);
  }
  if (!pricingCoverageKnown) {
    notes.push('Waiting for the run estimate before grading pricing coverage.');
  } else if (missingPricingCount) {
    notes.push(`${missingPricingCount} selected target(s) lack pricing, so cost comparison will be incomplete unless pricing is added.`);
  }

  const ready = hasBoth
    && !isConnectivitySmoke
    && promptPackEvidenceReady
    && enoughSelectedTasks
    && repetitions != null
    && repetitions >= recommendedTaskRepetitions
    && warmupRuns != null
    && warmupRuns >= 1
    && pricingCoverageKnown
    && missingPricingCount === 0;
  const facts = [
    `${selectedLocalCount} local selected`,
    `${selectedCloudCount} cloud selected`,
    formatEvidenceProfile(pack.evidenceProfile),
    formatCalibrationStatus(pack.calibrationStatus),
    `${selectedTaskCount}/${totalTaskCount || selectedTaskCount} task(s)`,
    `${repetitions ?? '-'} repetition(s)`,
    `${warmupRuns ?? '-'} warmup(s)`,
    `${concurrency ?? '-'} concurrent`,
    maxCostUsd == null ? 'no cost cap' : `${formatCost(maxCostUsd)} cap`,
    estimate ? `${estimate.pricedTargets}/${estimate.targetCount} priced` : 'pricing pending',
  ];

  return {
    tone: ready ? 'ok' : 'warn',
    headline: ready
      ? 'This run is set up for comparison-grade local/cloud evidence.'
      : 'This run is useful, but it is not yet comparison-grade evidence.',
    facts,
    notes,
    recommendedCostCapUsd,
    pricingRepairTargetIds,
  };
}

function runRepetitionConfidenceWarning(pack: BenchmarkPack, repetitions: number) {
  if (repetitions >= recommendedTaskRepetitions || !pack.taskTypes.includes('prompt')) {
    return '';
  }
  return `${repetitions} repetition(s) gives only ${repetitions} measured pass/fail sample(s) per task and target. Use at least ${recommendedTaskRepetitions} repetitions for model-selection confidence; one repetition is fine for connectivity smoke, but weak for choosing between local/cloud models.`;
}

function runScaleConfidenceWarning({
  measuredRuns,
  warmupCalls,
  targetCount,
  taskCount,
  repetitions,
  concurrency,
  wallClockTimeoutSeconds,
  heavy,
}: {
  measuredRuns: number;
  warmupCalls: number;
  targetCount: number;
  taskCount: number;
  repetitions: number;
  concurrency: number;
  wallClockTimeoutSeconds: number | null;
  heavy: boolean;
}) {
  if (!measuredRuns) {
    return '';
  }
  const manyRuns = measuredRuns >= 100;
  const manyCalls = measuredRuns + warmupCalls >= 150;
  const longWallClock = wallClockTimeoutSeconds != null && wallClockTimeoutSeconds >= 7200;
  if (!manyRuns && !manyCalls && !longWallClock && !heavy) {
    return '';
  }
  const parts = [
    `${formatInteger(measuredRuns)} measured run(s)`,
    warmupCalls ? `${formatInteger(warmupCalls)} warmup call(s)` : null,
    `${targetCount} target(s) x ${taskCount} task(s) x ${repetitions} repetition(s)`,
    `${concurrency} concurrent`,
    longWallClock ? `${formatDurationSeconds(wallClockTimeoutSeconds)} timeout envelope` : null,
    heavy ? 'heavy benchmark pack' : null,
  ].filter(Boolean);
  return `${parts.join('; ')}. This is a substantial benchmark job; narrow the scope or set a max-cost cap if this was not intentional.`;
}

function RunEstimatePanel({ estimate, error, fallbackRuns, fallbackWarmups, selectedTargets, packTasks, repetitions, concurrency, maxCostUsd }: { estimate: RunEstimate | null; error: string; fallbackRuns: number; fallbackWarmups: number; selectedTargets: number; packTasks: number; repetitions: number; concurrency: number; maxCostUsd?: number }) {
  if (error) {
    return <p className="muted">{fallbackRuns || 0} measured task run(s): {selectedTargets} target(s) x {packTasks} task(s) x {repetitions} repetition(s), up to {concurrency} at once. Estimate unavailable: {error}</p>;
  }
  if (!estimate) {
    return <p className="muted">{fallbackRuns || 0} measured task run(s): {selectedTargets} target(s) x {packTasks} task(s) x {repetitions} repetition(s), up to {concurrency} at once. {fallbackWarmups ? `${fallbackWarmups} warmup call(s) run first and are excluded from results.` : 'No warmup calls.'}</p>;
  }
  return <div className="preflight-box">
    <strong>Run estimate</strong>
    <div className="mini-grid">
      <span>{formatInteger(estimate.measuredRuns)} measured</span>
      <span>{formatInteger(estimate.warmupCalls)} warmup</span>
      <span>{formatInteger(estimate.totalModelCalls)} calls</span>
      <span>{estimate.concurrency} concurrent</span>
    </div>
    <div className="mini-grid">
      <span>{formatInteger(estimate.estimatedPromptTokens)} prompt tok</span>
      <span>{formatInteger(estimate.estimatedMaxCompletionTokens)} max out tok</span>
      <span>{estimate.estimatedMaxCostUsd == null ? 'cost unknown' : `${formatCost(estimate.estimatedMaxCostUsd)} max`}</span>
      <span>{maxCostUsd == null ? 'no cost cap' : `${formatCost(maxCostUsd)} cap`}</span>
      <span>{estimate.pricedTargets}/{estimate.targetCount} priced</span>
    </div>
    <div className="mini-grid">
      <span>{formatDurationSeconds(estimate.estimatedWallClockTimeoutSeconds)} wall max</span>
      <span>{formatDurationSeconds(estimate.estimatedMeasuredTimeoutSeconds)} measured cap</span>
      <span>{formatDurationSeconds(estimate.estimatedWarmupTimeoutSeconds)} warmup cap</span>
      <span>{estimate.heavy ? 'heavy pack' : 'normal pack'}</span>
    </div>
    {estimate.notes.length ? <p className="muted">{estimate.notes.join(' ')}</p> : null}
  </div>;
}

function applyRunBuilderIntentSettings(
  intent: RunBuilderIntent,
  setRepetitions: (value: string) => void,
  setWarmupRuns: (value: string) => void,
  setConcurrency: (value: string) => void,
  setMaxCostUsd: (value: string) => void,
) {
  const updates: string[] = [];
  const repetitions = boundedIntentInteger(intent.repetitions, 1, 100);
  const warmups = boundedIntentInteger(intent.warmupRuns, 0, 20);
  const concurrency = boundedIntentInteger(intent.concurrency, 1, 8);
  if (repetitions != null) {
    setRepetitions(String(repetitions));
    updates.push(`${repetitions} repetition(s)`);
  }
  if (warmups != null) {
    setWarmupRuns(String(warmups));
    updates.push(`${warmups} warmup(s)`);
  }
  if (concurrency != null) {
    setConcurrency(String(concurrency));
    updates.push(`${concurrency} concurrent`);
  }
  if (typeof intent.maxCostUsd === 'number' && Number.isFinite(intent.maxCostUsd) && intent.maxCostUsd >= 0) {
    setMaxCostUsd(String(intent.maxCostUsd));
    updates.push(`max cost ${formatCost(intent.maxCostUsd)}`);
  }
  return updates.length ? `Run Builder ready with ${updates.join(', ')}` : '';
}

function boundedIntentInteger(value: number | undefined, min: number, max: number) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  return Math.max(min, Math.min(max, Math.round(value)));
}

function runCostLimitMessage(estimate: RunEstimate | null, maxCostUsd?: number) {
  if (maxCostUsd == null || !estimate) {
    return '';
  }
  if (estimate.unpricedTargets.length) {
    return `Max cost cannot be enforced because ${estimate.unpricedTargets.length} selected target(s) have no pricing: ${estimate.unpricedTargets.join(', ')}`;
  }
  if (estimate.estimatedMaxCostUsd != null && estimate.estimatedMaxCostUsd > maxCostUsd) {
    return `Estimated max cost ${formatCost(estimate.estimatedMaxCostUsd)} exceeds cap ${formatCost(maxCostUsd)}`;
  }
  return '';
}

function blockingRunEstimateErrorMessage(error: string) {
  if (!error) {
    return '';
  }
  return [
    'target_preflight_failed',
    'incompatible_target',
    'target_not_found',
    'max_cost',
  ].some(prefix => error.startsWith(prefix))
    ? benchmarkRunFailureMessage(error)
    : '';
}

function mergeJob(jobs: RunJob[], next: RunJob) {
  const existing = jobs.filter(job => job.id !== next.id);
  return [next, ...existing].sort((a, b) => b.startedAt.localeCompare(a.startedAt));
}

function isJobActive(job: RunJob) {
  return job.status === 'queued' || job.status === 'running' || job.status === 'cancelling';
}

function isJobFinished(job: RunJob) {
  return job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled';
}

function isJobRetryable(job: RunJob) {
  return job.status === 'failed' || job.status === 'cancelled' || job.results.some(result => result.status !== 'passed');
}

function retryJobScopeLabel(sourceJob: RunJob | undefined, retryJob: RunJob) {
  const source = sourceJob?.settings;
  const retry = retryJob.settings;
  if (source && retry && (retry.targetCount < source.targetCount || retry.taskCount < source.taskCount || retry.repetitions < source.repetitions)) {
    return 'Scoped retry';
  }
  return 'Retry';
}

function jobPlanSummary(job: RunJob) {
  const settings = job.settings;
  if (!settings) {
    return '-';
  }
  const parts = [
    `${settings.targetCount} target(s)`,
    `${settings.taskCount} task(s)`,
    `${settings.repetitions} rep`,
    `${settings.warmupRuns} warmup`,
    `${settings.concurrency} concurrent`,
  ];
  if (settings.docker) {
    parts.push('Docker');
  }
  if (settings.maxCostUsd != null) {
    parts.push(`${formatCost(settings.maxCostUsd)} cap`);
  }
  if (settings.replay) {
    const mode = settings.replay.mode === 'duplicate' ? 'Duplicate' : 'Retry';
    const scope = settings.replay.scoped ? 'scoped' : 'full';
    parts.unshift(`${mode} of ${settings.replay.sourceJobId.slice(0, 8)} (${scope})`);
  }
  return parts.join(', ');
}

function jobStatusClass(status: string) {
  if (status === 'completed') {
    return 'ok';
  }
  if (status === 'failed') {
    return 'error';
  }
  return 'warn';
}

function JobMessageCell({ job }: { job: { message: string; error?: string | null } }) {
  const message = job.message || '-';
  const detail = job.error?.trim() || '';
  if (!detail || detail === message) {
    return <span title={message}>{message}</span>;
  }
  return <span title={`${message}: ${detail}`}>{message}<span className="muted result-error-detail">{truncateErrorDetail(detail)}</span></span>;
}

function Results({ results, targets, adapters, packs, artifacts, selectedRunId, setSelectedRunId, selectedResult, artifactText, setArtifactText, setMessage, scopeIntent, openRunBuilder, openTargetRepair }: {
  results: RunResult[];
  targets: Target[];
  adapters: Adapter[];
  packs: BenchmarkPack[];
  artifacts: Artifact[];
  selectedRunId: string;
  setSelectedRunId: (id: string) => void;
  selectedResult?: RunResult;
  artifactText: string;
  setArtifactText: (text: string) => void;
  setMessage: (message: string) => void;
  scopeIntent: ResultsScopeIntent | null;
  openRunBuilder: (intent: RunBuilderIntent) => void;
  openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void;
}) {
  const [statusFilter, setStatusFilter] = useState('all');
  const [packFilter, setPackFilter] = useState('all');
  const [targetFilter, setTargetFilter] = useState('all');
  const [groupFilter, setGroupFilter] = useState('all');
  const [providerFilter, setProviderFilter] = useState('all');
  const [providerModelFilter, setProviderModelFilter] = useState('all');
  const [dateWindow, setDateWindow] = useState<DateWindow>('all');
  const [selectedArtifactId, setSelectedArtifactId] = useState('');
  const adapterById = useMemo(() => new Map(adapters.map(adapter => [adapter.id, adapter])), [adapters]);
  const packOptions = useMemo(() => uniqueSorted(results.map(resultPackId)), [results]);
  const targetOptions = useMemo(() => uniqueSorted(results.map(resultTargetId)), [results]);
  const groupOptions = useMemo(() => uniqueSorted(results.map(resultGroupId)), [results]);
  const providerOptions = useMemo(() => buildProviderOptions(results, adapterById), [results, adapterById]);
  const providerModelOptions = useMemo(() => buildProviderModelOptions(results), [results]);
  const statusOptions = useMemo(() => uniqueSorted(results.map(result => result.status)), [results]);
  const activeScopeFilters = useMemo(() => resultScopeFilters({
    statusFilter,
    packFilter,
    targetFilter,
    groupFilter,
    providerFilter,
    providerModelFilter,
    dateWindow,
    providerOptions,
    providerModelOptions,
  }), [statusFilter, packFilter, targetFilter, groupFilter, providerFilter, providerModelFilter, dateWindow, providerOptions, providerModelOptions]);
  const visibleResults = useMemo(() => results.filter(result => {
    if (statusFilter !== 'all' && result.status !== statusFilter) {
      return false;
    }
    if (packFilter !== 'all' && resultPackId(result) !== packFilter) {
      return false;
    }
    if (targetFilter !== 'all' && resultTargetId(result) !== targetFilter) {
      return false;
    }
    if (groupFilter !== 'all' && resultGroupId(result) !== groupFilter) {
      return false;
    }
    if (providerFilter !== 'all' && resultProviderKey(result) !== providerFilter) {
      return false;
    }
    if (providerModelFilter !== 'all' && resultProviderModelKey(result) !== providerModelFilter) {
      return false;
    }
    if (!resultMatchesDateWindow(result, dateWindow)) {
      return false;
    }
    return true;
  }), [results, statusFilter, packFilter, targetFilter, groupFilter, providerFilter, providerModelFilter, dateWindow]);
  const visibleRunIds = useMemo(() => visibleResults.map(result => result.id), [visibleResults]);
  const scopedExportRunIds = activeScopeFilters.length ? visibleRunIds : undefined;
  const filteredScopeWarning = resultScopeWarning(activeScopeFilters, visibleResults.length, results.length, statusFilter, targetFilter);
  const comparisonRows = useMemo(() => buildComparisonRows(visibleResults, adapterById), [visibleResults, adapterById]);
  const modelIdentityWarnings = useMemo(() => buildModelIdentityWarnings(comparisonRows), [comparisonRows]);
  const generationSettingWarnings = useMemo(() => buildGenerationSettingWarnings(comparisonRows), [comparisonRows]);
  const packEvidenceIssues = useMemo(() => buildPackEvidenceIssues(visibleResults, packs), [visibleResults, packs]);
  const packCalibrationIssues = useMemo(() => buildPackCalibrationIssues(visibleResults), [visibleResults]);
  const runGroupTrendRows = useMemo(() => buildRunGroupTrendRows(comparisonRows), [comparisonRows]);
  const taskRows = useMemo(() => buildTaskComparisonRows(visibleResults), [visibleResults]);
  const taskTargetMatrix = useMemo(() => buildTaskTargetMatrix(visibleResults), [visibleResults]);
  const targetRankingRows = useMemo(() => buildTargetRankingRows(visibleResults, adapterById), [visibleResults, adapterById]);
  const decision = useMemo(() => buildDecisionSnapshot(comparisonRows, taskRows, targetRankingRows, packEvidenceIssues, packCalibrationIssues), [comparisonRows, taskRows, targetRankingRows, packEvidenceIssues, packCalibrationIssues]);
  const resultEvidence = useMemo(() => buildResultEvidenceSummary(comparisonRows, taskRows, targetRankingRows, targets, packs, packEvidenceIssues, packCalibrationIssues), [comparisonRows, taskRows, targetRankingRows, targets, packs, packEvidenceIssues, packCalibrationIssues]);
  const summary = useMemo(() => buildResultSummary(visibleResults), [visibleResults]);
  const metricCoverageRows = useMemo(() => buildMetricCoverageRows(visibleResults), [visibleResults]);
  const errorRows = useMemo(() => buildErrorRows(visibleResults), [visibleResults]);
  const errorRecoveryRows = useMemo(() => buildErrorRecoveryRows(visibleResults), [visibleResults]);
  const scoreDistributionRows = useMemo(() => buildScoreDistributionRows(visibleResults), [visibleResults]);
  const errorChartRows = useMemo(() => errorRows.map(row => ({
    label: row.code,
    value: row.count,
    valueLabel: String(row.count),
    tone: 'error',
  })), [errorRows]);
  const sortedArtifacts = useMemo(() => [...artifacts].sort(compareArtifacts), [artifacts]);
  const selectedArtifact = sortedArtifacts.find(artifact => artifact.id === selectedArtifactId);
  useEffect(() => {
    if (!scopeIntent) {
      return;
    }
    setStatusFilter('all');
    setPackFilter('all');
    setTargetFilter('all');
    setProviderFilter('all');
    setProviderModelFilter('all');
    setDateWindow('all');
    setGroupFilter(scopeIntent.groupId);
    const matchingRunId = scopeIntent.runId
      && results.some(result => result.id === scopeIntent.runId && resultGroupId(result) === scopeIntent.groupId)
      ? scopeIntent.runId
      : results.find(result => resultGroupId(result) === scopeIntent.groupId)?.id;
    if (matchingRunId) {
      setSelectedRunId(matchingRunId);
    }
  }, [scopeIntent, results, setSelectedRunId]);
  useEffect(() => {
    let cancelled = false;
    const artifact = pickPreferredArtifact(sortedArtifacts);
    setSelectedArtifactId(artifact?.id ?? '');
    setArtifactText('');
    if (!artifact) {
      return () => {
        cancelled = true;
      };
    }
    readArtifact(artifact.path)
      .then(text => {
        if (!cancelled) {
          setArtifactText(formatArtifactText(artifact, text));
        }
      })
      .catch(error => {
        if (!cancelled) {
          setMessage(String(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedRunId, sortedArtifacts, setArtifactText, setMessage]);
  async function openArtifact(artifact: Artifact) {
    try {
      setSelectedArtifactId(artifact.id);
      setArtifactText(formatArtifactText(artifact, await readArtifact(artifact.path)));
    } catch (error) {
      setMessage(String(error));
    }
  }
  async function copyArtifact() {
    if (!artifactText || !selectedArtifact) {
      return;
    }
    await navigator.clipboard.writeText(artifactText);
    setMessage(`${artifactDisplayName(selectedArtifact)} copied`);
  }
  async function copyExport(format: 'jsonl' | 'csv' | 'markdown' | 'analysis') {
    const text = await exportResults(format, scopedExportRunIds);
    await navigator.clipboard.writeText(text);
    const scopeNote = scopedExportRunIds ? ' from the filtered scope' : '';
    setMessage(`${exportFormatLabel(format)} export copied for ${visibleRunIds.length} result(s)${scopeNote}`);
  }
  async function createReportFolder() {
    const path = await exportReportFolder(scopedExportRunIds);
    await navigator.clipboard.writeText(path);
    const scopeNote = scopedExportRunIds ? ' from the filtered scope' : '';
    setMessage(`Report folder created for ${visibleRunIds.length} result(s)${scopeNote}: ${path}`);
  }
  function clearResultFilters() {
    setStatusFilter('all');
    setPackFilter('all');
    setTargetFilter('all');
    setGroupFilter('all');
    setProviderFilter('all');
    setProviderModelFilter('all');
    setDateWindow('all');
    setSelectedRunId(results[0]?.id ?? '');
    setMessage('Results filters cleared; charts, rankings, recommendations, and exports now use all result rows');
  }
  function openEvidenceNextRun(intent: RunBuilderIntent) {
    openRunBuilder(intent);
    const taskNote = intent.taskIds?.length ? `, ${intent.taskIds.length} task(s)` : '';
    setMessage(`Run Builder ready for evidence follow-up: ${intent.targetIds.length} target(s)${taskNote}, ${intent.benchmarkPackId ?? 'recommended pack'}, ${intent.repetitions ?? 3} repetition(s), ${intent.warmupRuns ?? 1} warmup(s)`);
  }
  function openEvidencePricingRepair(targetIds: string[]) {
    openTargetRepair({ targetIds, code: 'pricing_assumption' });
    setMessage(`${errorCategoryRepairHint('pricing_assumption')} Affected target(s): ${previewList(targetIds)}`);
  }
  function openErrorRecoveryRun(row: ErrorRecoveryRow) {
    if (row.packIds.length !== 1) {
      setMessage(`Filter Results to a single benchmark pack before opening a scoped rerun for ${row.code}`);
      return;
    }
    const targetIds = row.targetIds.filter(id => id && id !== '-');
    if (!targetIds.length) {
      setMessage(`No target IDs are available for ${row.code}`);
      return;
    }
    const taskIds = row.taskIds.filter(id => id && id !== '-');
    const availability = errorRecoveryTargetAvailability(row, targets);
    if (!availability.selectableTargetIds.length) {
      const skipped = previewList([...availability.unavailableTargetIds, ...availability.missingTargetIds]);
      setMessage(`No selectable targets are available for ${row.code} recovery. Fix or recreate target(s): ${skipped}`);
      return;
    }
    const selectedTargets = targets.filter(target => availability.selectableTargetIds.includes(target.id));
    const includesCloudTarget = selectedTargets.some(isCloudModelTarget);
    openRunBuilder({
      targetIds: availability.selectableTargetIds,
      benchmarkPackId: row.packIds[0],
      taskIds: taskIds.length ? taskIds : undefined,
      repetitions: 1,
      warmupRuns: 0,
      concurrency: Math.min(2, Math.max(1, availability.selectableTargetIds.length)),
      maxCostUsd: includesCloudTarget ? defaultComparisonMaxCostUsd : undefined,
    });
    const taskNote = taskIds.length ? `, ${taskIds.length} task(s)` : '';
    const costNote = includesCloudTarget ? `, ${formatCost(defaultComparisonMaxCostUsd)} cap` : '';
    const skipped = [...availability.unavailableTargetIds, ...availability.missingTargetIds];
    const skippedNote = skipped.length ? `. Skipped unavailable target(s): ${previewList(skipped)}` : '';
    setMessage(`Run Builder ready for ${row.code} recovery: ${availability.selectableTargetIds.length} target(s)${taskNote}, ${row.packIds[0]}, 1 repetition${costNote}${skippedNote}`);
  }
  function openErrorRecoveryRepair(row: ErrorRecoveryRow) {
    const targetIds = row.targetIds.filter(id => id && id !== '-');
    openTargetRepair({ targetIds, code: row.code });
    const targetNote = targetIds.length ? ` for ${previewList(targetIds)}` : '';
    setMessage(`${errorCategoryRepairHint(row.code)}${targetNote}`);
  }
  return <section><div className="section-head"><h1>Results</h1><div className="actions"><button onClick={() => copyExport('jsonl').catch(error => setMessage(String(error)))}><Download size={16} />JSONL</button><button onClick={() => copyExport('csv').catch(error => setMessage(String(error)))}><Download size={16} />CSV</button><button onClick={() => copyExport('analysis').catch(error => setMessage(String(error)))}><Download size={16} />Analysis JSON</button><button onClick={() => copyExport('markdown').catch(error => setMessage(String(error)))}><Download size={16} />Report</button><button onClick={() => createReportFolder().catch(error => setMessage(String(error)))}><Download size={16} />Folder</button></div></div>
    <div className="filter-grid">
      <label>Pack <select value={packFilter} onChange={event => setPackFilter(event.target.value)}><option value="all">All</option>{packOptions.map(pack => <option key={pack} value={pack}>{pack}</option>)}</select></label>
      <label>Target <select value={targetFilter} onChange={event => setTargetFilter(event.target.value)}><option value="all">All</option>{targetOptions.map(target => <option key={target} value={target}>{target}</option>)}</select></label>
      <label>Group <select value={groupFilter} onChange={event => setGroupFilter(event.target.value)}><option value="all">All</option>{groupOptions.map(group => <option key={group} value={group}>{group.slice(0, 8)}</option>)}</select></label>
      <label>Provider <select value={providerFilter} onChange={event => setProviderFilter(event.target.value)}><option value="all">All</option>{providerOptions.map(provider => <option key={provider.key} value={provider.key}>{provider.label}</option>)}</select></label>
      <label>Model <select value={providerModelFilter} onChange={event => setProviderModelFilter(event.target.value)}><option value="all">All</option>{providerModelOptions.map(model => <option key={model.key} value={model.key}>{model.label}</option>)}</select></label>
      <label>Status <select value={statusFilter} onChange={event => setStatusFilter(event.target.value)}><option value="all">All</option>{statusOptions.map(status => <option key={status} value={status}>{status}</option>)}</select></label>
      <label>Date <select value={dateWindow} onChange={event => setDateWindow(event.target.value as DateWindow)}>{dateWindowOptions.map(option => <option key={option.id} value={option.id}>{option.label}</option>)}</select></label>
    </div>
    {filteredScopeWarning ? <div className="preflight-box warn"><strong>Filtered Result Scope</strong><p>{filteredScopeWarning}</p><div className="row-actions"><button onClick={clearResultFilters}><RotateCcw size={14} />Clear filters</button></div></div> : null}
    {resultEvidence ? <ResultEvidencePanel evidence={resultEvidence} onOpenNextRun={openEvidenceNextRun} onRepairPricingAssumptions={openEvidencePricingRepair} /> : null}
    {packEvidenceIssues.length ? <PackEvidenceWarningsPanel issues={packEvidenceIssues} /> : null}
    {packCalibrationIssues.length ? <PackCalibrationWarningsPanel issues={packCalibrationIssues} /> : null}
    {modelIdentityWarnings.length ? <ModelIdentityWarningsPanel warnings={modelIdentityWarnings} onSelectRun={setSelectedRunId} selectedRunId={selectedRunId} /> : null}
    {generationSettingWarnings.length ? <GenerationSettingWarningsPanel warnings={generationSettingWarnings} onSelectRun={setSelectedRunId} selectedRunId={selectedRunId} /> : null}
    <div className="kpi-grid">
      <Card title="Runs" value={summary.total} note={`${summary.passed} passed`} />
      <Card title="Pass rate" value={formatPercent(summary.passRate)} note={`${summary.failed} non-passing`} />
      <Card title="Avg score" value={formatNumber(summary.avgScore)} note={`p95 ${formatMs(summary.p95TimeMs)}`} />
      <Card title="Total cost" value={formatCost(summary.totalCostUsd)} note={`${formatInteger(summary.totalTokens)} tokens`} />
    </div>
    <MetricCoveragePanel rows={metricCoverageRows} total={visibleResults.length} />
    {comparisonRows.length ? <div className="analysis-grid">
      <ChartPanel title="Pass Rate" rows={comparisonRows.slice(0, 8).map(row => ({ label: chartRowLabel(row), value: row.passRate, valueLabel: `${row.passed}/${row.runs}`, max: 1, tone: row.passRate >= 0.9 ? 'ok' : row.passRate >= 0.5 ? 'warn' : 'error' }))} />
      <ChartPanel title="Latency P95" rows={comparisonRows.filter(row => row.p95TimeMs != null).slice(0, 8).map(row => ({ label: chartRowLabel(row), value: row.p95TimeMs ?? 0, valueLabel: formatMs(row.p95TimeMs), tone: 'latency' }))} />
      <ChartPanel title="Output Tok/s" rows={comparisonRows.filter(row => row.avgOutputTokensPerSecond != null).slice(0, 8).map(row => ({ label: chartRowLabel(row), value: row.avgOutputTokensPerSecond ?? 0, valueLabel: formatNumber(row.avgOutputTokensPerSecond), tone: 'throughput' }))} empty="No throughput metrics" />
      <ChartPanel title="Cost" rows={comparisonRows.filter(row => row.avgCostUsd != null).slice(0, 8).map(row => ({ label: chartRowLabel(row), value: row.avgCostUsd ?? 0, valueLabel: formatCost(row.avgCostUsd), tone: 'cost' }))} empty="No cost metrics" />
    </div> : null}
    <div className="analysis-grid">
      <ChartPanel title="Score Distribution" rows={scoreDistributionRows} empty="No scored runs in scope" />
      <ChartPanel title="Error Categories" rows={errorChartRows} empty="No non-passing results in scope" />
    </div>
    {errorRecoveryRows.length ? <div className="panel error-recovery-panel"><div className="panel-head"><h2>Error Recovery</h2><span className="status-text">{errorRecoveryRows.length} categor{errorRecoveryRows.length === 1 ? 'y' : 'ies'} in scope</span></div>
      <table className="error-recovery-table">
        <thead><tr><th>Error</th><th>Runs</th><th>Scope</th><th>HTTP</th><th>Retry</th><th>Recovery</th><th>Example</th><th></th></tr></thead>
        <tbody>{errorRecoveryRows.map(row => {
          const availability = errorRecoveryTargetAvailability(row, targets);
          const skippedTargets = [...availability.unavailableTargetIds, ...availability.missingTargetIds];
          const rerunDisabled = row.packIds.length !== 1 || !availability.selectableTargetIds.length;
          const canRepairTarget = errorCategoryHasTargetRepair(row.code);
          return <tr key={row.key}>
            <td><strong>{row.code}</strong></td>
            <td>{row.count}</td>
            <td><div>Packs: {previewList(row.packIds)}</div><div>Targets: {previewList(row.targetIds)}</div>{skippedTargets.length ? <div className="muted">Unavailable: {previewList(skippedTargets)}</div> : null}<div className="muted">Tasks: {previewList(row.taskIds)}</div></td>
            <td>{row.httpStatuses}</td>
            <td><span className={`pill ${row.retryable ? 'warn' : 'unknown'}`}>{row.retryable ? 'retryable' : 'manual'}</span></td>
            <td className="recovery-hint-cell">{row.recoveryHint}</td>
            <td className="muted recovery-example-cell">{row.exampleDetail || '-'}</td>
            <td><div className="row-actions error-recovery-actions">{canRepairTarget ? <button title={errorCategoryRepairHint(row.code)} onClick={() => openErrorRecoveryRepair(row)}><Wrench size={14} />Repair</button> : null}<button disabled={rerunDisabled} title={errorRecoveryRerunTitle(row, availability)} onClick={() => openErrorRecoveryRun(row)}><RotateCcw size={14} />Rerun</button></div></td>
          </tr>;
        })}</tbody>
      </table>
    </div> : null}
    {decision ? <DecisionSnapshotPanel decision={decision} onSelectRun={setSelectedRunId} selectedRunId={selectedRunId} /> : null}
    {runGroupTrendRows.length ? <>
      <h2>Run Group Trends</h2>
      <table><thead><tr><th>Pack</th><th>Target</th><th>Current</th><th>Previous</th><th>Runs</th><th>Pass rate delta</th><th>Score delta</th><th>P95 delta</th><th>Avg cost delta</th><th>Signal</th></tr></thead><tbody>{runGroupTrendRows.map(row => <tr key={row.key} onClick={() => setSelectedRunId(row.current.firstRunId)} className={row.current.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{row.packId}</td><td>{row.targetId}</td><td>{row.current.groupId.slice(0, 8)}</td><td>{row.previous.groupId.slice(0, 8)}</td><td>{row.current.runs}/{row.previous.runs}</td><td>{formatPercentPointDelta(row.passRateDelta)}</td><td>{formatNumberDelta(row.avgScoreDelta)}</td><td>{formatMsDelta(row.p95TimeDeltaMs)}</td><td>{formatCostDelta(row.avgCostDeltaUsd)}</td><td><span className={`pill ${row.signalLevel}`}>{row.signal}</span></td></tr>)}</tbody></table>
    </> : null}
    {targetRankingRows.length ? <>
      <h2>Target Ranking</h2>
      <table><thead><tr><th>Rank</th><th>Target</th><th>Provider</th><th>Scope</th><th>Runs</th><th>Weighted pass</th><th>Pass rate / 95% CI</th><th>Weighted score</th><th>Score avg / σ</th><th>P95 wall</th><th>Avg cost</th><th>Out tok/s</th><th>Errors</th></tr></thead><tbody>{targetRankingRows.map((row, index) => <tr key={row.targetId} onClick={() => setSelectedRunId(row.firstRunId)} className={row.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{index + 1}</td><td>{row.targetId}</td><td>{row.providers}</td><td>{row.packs} pack(s), {row.tasks} task(s), {row.groups} group(s)</td><td>{row.runs}</td><td>{formatPercent(row.weightedPassRate)}</td><td>{row.passed}/{row.runs} ({formatPercent(row.passRate)}; {formatPercentRange(row.passRateCiLow, row.passRateCiHigh)})</td><td>{formatNumber(row.weightedAvgScore)}</td><td>{formatNumberWithSpread(row.avgScore, row.scoreStdDev)}</td><td>{formatMs(row.p95TimeMs)}</td><td>{formatCost(row.avgCostUsd)}</td><td>{formatNumber(row.avgOutputTokensPerSecond)}</td><td>{row.errorCodes}</td></tr>)}</tbody></table>
    </> : null}
    {targetRankingRows.length ? <DistributionSummaryPanel targets={targetRankingRows} tasks={taskRows} onSelectRun={setSelectedRunId} selectedRunId={selectedRunId} /> : null}
    {taskRows.length ? <div className="analysis-grid">
      <ChartPanel title="Worst Tasks" rows={taskRows.slice(0, 8).map(row => ({ label: taskRowLabel(row), value: 1 - row.passRate, valueLabel: `${row.passed}/${row.runs}`, max: 1, tone: row.passRate >= 0.9 ? 'ok' : row.passRate >= 0.5 ? 'warn' : 'error' }))} />
      <ChartPanel title="Task Latency P95" rows={taskRows.filter(row => row.p95TimeMs != null).slice(0, 8).map(row => ({ label: taskRowLabel(row), value: row.p95TimeMs ?? 0, valueLabel: formatMs(row.p95TimeMs), tone: 'latency' }))} empty="No task latency metrics" />
    </div> : null}
    {taskTargetMatrix.rows.length ? <>
      <h2>Task Target Matrix</h2>
      <table className="task-matrix"><thead><tr><th>Group</th><th>Pack</th><th>Task</th>{taskTargetMatrix.targets.map(target => <th key={target}>{target}</th>)}</tr></thead><tbody>{taskTargetMatrix.rows.map(row => <tr key={row.key}><td>{row.groupId.slice(0, 8)}</td><td>{row.packId}</td><td>{row.taskId}</td>{taskTargetMatrix.targets.map(target => {
        const cell = row.cells[target];
        if (!cell) {
          return <td key={target} className="muted">-</td>;
        }
        return <td key={target} className={`task-matrix-cell ${cell.passRate >= 0.9 ? 'ok' : cell.passRate >= 0.5 ? 'warn' : 'error'}`} onClick={() => setSelectedRunId(cell.firstRunId)}><strong>{cell.passed}/{cell.runs}</strong><span>score {formatNumber(cell.avgScore)}</span><span>p95 {formatMs(cell.p95TimeMs)}</span>{cell.errorCodes !== '-' ? <span>errors {cell.errorCodes}</span> : null}</td>;
      })}</tr>)}</tbody></table>
    </> : null}
    <div className="analysis-grid narrow">
      <div className="panel"><h2>Error Table</h2>{errorRows.length ? <table className="compact-table"><tbody>{errorRows.map(row => <tr key={row.code}><td><strong>{row.code}</strong>{row.examples.length ? <div className="muted">{row.examples.join(' · ')}</div> : null}</td><td>{row.count}</td></tr>)}</tbody></table> : <p className="muted">No non-passing results in scope.</p>}</div>
      <div className="panel"><h2>Scope</h2><table className="compact-table"><tbody><tr><td>Packs</td><td>{packFilter === 'all' ? packOptions.length : 1}</td></tr><tr><td>Targets</td><td>{targetFilter === 'all' ? targetOptions.length : 1}</td></tr><tr><td>Groups</td><td>{groupFilter === 'all' ? groupOptions.length : 1}</td></tr><tr><td>Providers</td><td>{providerFilter === 'all' ? providerOptions.length : 1}</td></tr><tr><td>Models</td><td>{providerModelFilter === 'all' ? providerModelOptions.length : 1}</td></tr><tr><td>Date</td><td>{dateWindowLabel(dateWindow)}</td></tr><tr><td>Rows</td><td>{visibleResults.length}</td></tr></tbody></table></div>
    </div>
    {comparisonRows.length ? <>
      <h2>Comparison</h2>
      <table><thead><tr><th>Group</th><th>Pack</th><th>Target</th><th>Provider</th><th>Latest</th><th>Runs</th><th>Pass rate</th><th>Score avg / σ</th><th>Time avg / p95</th><th>TTFB</th><th>TTFT</th><th>Req</th><th>Avg tokens</th><th>Reasoning</th><th>Out tok/s</th><th>Avg attempts</th><th>HTTP</th><th>Model</th><th>Source</th><th>Generation</th><th>Finish</th><th>Pricing</th><th>Avg cost</th></tr></thead><tbody>{comparisonRows.map(row => <tr key={row.key} onClick={() => setSelectedRunId(row.firstRunId)} className={row.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{row.groupId.slice(0, 8)}</td><td>{row.packId}</td><td>{row.targetId}</td><td>{row.providers}</td><td>{formatDateTime(row.latestStarted)}</td><td>{row.runs}</td><td>{row.passed}/{row.runs} ({formatPercent(row.passRate)})</td><td>{formatNumberWithSpread(row.avgScore, row.scoreStdDev)}</td><td>{formatMsPair(row.avgTimeMs, row.p95TimeMs)}</td><td>{formatMs(row.avgProviderTimeToFirstByteMs)}</td><td>{formatMs(row.avgProviderTimeToFirstTokenMs)}</td><td>{formatMs(row.avgProviderRequestTotalMs)}</td><td>{row.avgTokens == null ? '-' : Math.round(row.avgTokens)}</td><td>{formatNumber(row.avgReasoningTokens)}</td><td>{formatNumber(row.avgOutputTokensPerSecond)}</td><td>{formatNumber(row.avgAttempts)}</td><td>{row.httpStatuses}</td><td>{row.providerModels}</td><td>{row.providerModelSources}</td><td>{row.generationSettings}</td><td>{row.finishReasons}</td><td>{row.pricingAssumptions}</td><td>{formatCost(row.avgCostUsd)}</td></tr>)}</tbody></table>
    </> : null}
    {taskRows.length ? <>
      <h2>Task Drilldown</h2>
      <table><thead><tr><th>Group</th><th>Pack</th><th>Task</th><th>Target</th><th>Runs</th><th>Pass rate</th><th>Score avg / σ</th><th>Time avg / p95</th><th>TTFT</th><th>Avg tokens</th><th>Out tok/s</th><th>HTTP</th><th>Error</th><th>Avg cost</th></tr></thead><tbody>{taskRows.map(row => <tr key={row.key} onClick={() => setSelectedRunId(row.firstRunId)} className={row.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{row.groupId.slice(0, 8)}</td><td>{row.packId}</td><td>{row.taskId}</td><td>{row.targetId}</td><td>{row.runs}</td><td>{row.passed}/{row.runs} ({formatPercent(row.passRate)})</td><td>{formatNumberWithSpread(row.avgScore, row.scoreStdDev)}</td><td>{formatMsPair(row.avgTimeMs, row.p95TimeMs)}</td><td>{formatMs(row.avgProviderTimeToFirstTokenMs)}</td><td>{formatInteger(row.avgTokens)}</td><td>{formatNumber(row.avgOutputTokensPerSecond)}</td><td>{row.httpStatuses}</td><td>{row.errorCodes}</td><td>{formatCost(row.avgCostUsd)}</td></tr>)}</tbody></table>
    </> : null}
    <h2>Runs</h2>
    <table>
      <thead>
        <tr>
          <th>Run</th><th>Started</th><th>Group</th><th>Target</th><th>Provider</th><th>Task</th><th>Status</th><th>Error</th>
          <th>Safety</th><th>Files</th><th>Import</th><th>Parser</th><th>Score</th><th>Time</th><th>Setup</th><th>Target</th><th>Eval</th><th>Model call</th>
          <th>Exit</th><th>Harness</th><th>Stdout</th><th>Stderr</th><th>Files Δ</th><th>+ Lines</th><th>- Lines</th><th>Commands</th><th>Danger</th><th>TTFB</th><th>TTFT</th><th>Req</th><th>Tokens</th>
          <th>Reasoning</th><th>Cached</th><th>Cache read</th><th>Cache write</th><th>Out tok/s</th><th>Peak RSS</th><th>Attempts</th><th>Retry after</th><th>Retry delay</th><th>HTTP</th><th>Model</th><th>Source</th><th>Finish</th><th>Pricing</th><th>Cost</th>
        </tr>
      </thead>
      <tbody>
        {visibleResults.length ? visibleResults.map(result => (
          <tr key={result.id} className={result.id === selectedRunId ? 'selected-row' : ''} onClick={() => setSelectedRunId(result.id)}>
            <td>{result.id.slice(0, 8)}</td>
            <td>{formatDateTime(result.started_at)}</td>
            <td>{resultGroupId(result) === result.id ? '-' : resultGroupId(result).slice(0, 8)}</td>
            <td>{resultTargetId(result)}</td>
            <td>{resultProviderLabel(result, adapterById)}</td>
            <td>{result.taskId ?? result.task_id}</td>
            <td><span className={`pill ${result.status === 'passed' ? 'ok' : result.status === 'failed' ? 'error' : 'warn'}`}>{result.status}</span></td>
            <td><ResultErrorCell result={result} /></td>
            <td>{formatInteger(result.security_finding_count ?? null)}</td>
            <td>{formatInteger(result.security_files_scanned ?? null)}</td>
            <td>{formatImportProvenance(result)}</td>
            <td>{result.summary_source ?? '-'}</td>
            <td>{result.score ?? '-'}</td>
            <td>{Math.round(resultWallTimeMs(result) ?? 0)} ms</td>
            <td>{formatMs(result.setup_time_ms ?? null)}</td>
            <td>{formatMs(result.target_time_ms ?? null)}</td>
            <td>{formatMs(result.evaluation_time_ms ?? null)}</td>
            <td>{formatMs(result.model_call_wall_time_ms ?? null)}</td>
            <td>{formatInteger(result.exit_code ?? null)}</td>
            <td>{formatInteger(result.harness_exit_code ?? null)}</td>
            <td>{typeof result.stdout_bytes === 'number' ? formatBytes(result.stdout_bytes) : '-'}</td>
            <td>{typeof result.stderr_bytes === 'number' ? formatBytes(result.stderr_bytes) : '-'}</td>
            <td>{formatInteger(result.files_changed ?? null)}</td>
            <td>{formatInteger(result.lines_added ?? null)}</td>
            <td>{formatInteger(result.lines_deleted ?? null)}</td>
            <td>{formatInteger(result.commands_observed_count ?? null)}</td>
            <td>{formatInteger(result.dangerous_command_hits ?? null)}</td>
            <td>{formatMs(result.provider_time_to_first_byte_ms ?? null)}</td>
            <td>{formatMs(result.provider_time_to_first_token_ms ?? null)}</td>
            <td>{formatMs(result.provider_request_total_ms ?? null)}</td>
            <td>{formatTokenSummary(result)}</td>
            <td>{formatInteger(result.reasoning_tokens ?? null)}</td>
            <td>{formatInteger(result.cached_tokens ?? null)}</td>
            <td>{formatInteger(result.cache_read_tokens ?? null)}</td>
            <td>{formatInteger(result.cache_write_tokens ?? null)}</td>
            <td>{formatNumber(result.output_tokens_per_second ?? null)}</td>
            <td>{formatNumber(result.peak_rss_mb ?? null)}</td>
            <td>{result.provider_attempts ?? '-'}</td>
            <td>{formatMs(result.provider_retry_after_ms ?? null)}</td>
            <td>{formatMs(result.provider_retry_delay_ms ?? null)}</td>
            <td>{formatHttpStatus(result.http_status)}</td>
            <td>{result.provider_model ?? '-'}</td>
            <td>{result.provider_model_source ?? '-'}</td>
            <td>{result.finish_reason ?? '-'}</td>
            <td>{result.pricing_assumption ?? '-'}</td>
            <td>{formatCost(resultCostUsdForCoverage(result))}</td>
          </tr>
        )) : <tr><td colSpan={46} className="muted">No results match the current filters.</td></tr>}
      </tbody>
    </table>
    {selectedResult && <div className="detail-grid">
      <div className="panel"><div className="panel-head"><h2>Artifacts</h2>{selectedArtifact ? <button onClick={() => copyArtifact().catch(error => setMessage(String(error)))}><Copy size={14} />Copy</button> : null}</div>{sortedArtifacts.length ? <div className="artifact-list">{sortedArtifacts.map(artifact => <button key={artifact.id} className={`artifact-item ${artifact.id === selectedArtifactId ? 'active' : ''}`} onClick={() => openArtifact(artifact).catch(error => setMessage(String(error)))}><span><strong>{artifactLabel(artifact)}</strong><span className="muted">{artifactDisplayName(artifact)}</span></span><span>{formatBytes(artifact.size_bytes ?? 0)}</span></button>)}</div> : <p className="muted">No artifacts stored for this run.</p>}{selectedArtifact ? <table className="compact-table artifact-meta"><tbody><tr><td>Kind</td><td>{selectedArtifact.kind}</td></tr><tr><td>File</td><td>{artifactDisplayName(selectedArtifact)}</td></tr><tr><td>SHA-256</td><td>{selectedArtifact.sha256 ? shortHash(selectedArtifact.sha256) : '-'}</td></tr></tbody></table> : null}</div>
      <div className="panel"><h2>Metadata</h2><pre>{JSON.stringify(selectedResult.reproducibility ?? {}, null, 2)}</pre></div>
    </div>}
    {selectedArtifact && <div className="panel artifact-panel"><div className="panel-head"><h2>{artifactLabel(selectedArtifact)}</h2><span className="muted">{artifactDisplayName(selectedArtifact)}</span></div>{artifactText ? <ArtifactContent artifact={selectedArtifact} text={artifactText} /> : <p className="muted">Artifact is empty.</p>}</div>}
  </section>;
}

function Doctor({ checks, diagnostics, targets, adapters, packs, onRefresh, setBusy, setMessage, setPage, openRunBuilder, openTargetRepair, openTargetSetup, openHuggingFaceLocalSetup }: { checks: DoctorCheck[]; diagnostics: DiagnosticRecord[]; targets: Target[]; adapters: Adapter[]; packs: BenchmarkPack[]; onRefresh: () => Promise<void>; setBusy: (busy: boolean) => void; setMessage: (message: string) => void; setPage: (page: Page) => void; openRunBuilder: (intent: RunBuilderIntent) => void; openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void; openTargetSetup: (intent: Omit<TargetSetupIntent, 'nonce'>) => void; openHuggingFaceLocalSetup: (intent?: Omit<HuggingFaceLocalSetupIntent, 'nonce'>) => void }) {
  const [actionBusy, setActionBusy] = useState('');
  const [actionLog, setActionLog] = useState('');
  const errors = checks.filter(c => c.status === 'error').length;
  const warnings = checks.filter(c => c.status === 'warn').length;
  const ok = checks.filter(c => c.status === 'ok').length;
  const installableLocalMissing = checks.some(check => isLocalModelToolCheck(check) && check.status !== 'ok');
  const recommendedTargets = dashboardLocalCloudComparisonTargets(targets);
  const recommendedTargetIds = recommendedTargets.runTargetIds;
  const setupLocalTargetIds = recommendedTargets.setupLocalTargetIds;
  const setupCloudTargetIds = recommendedTargets.setupCloudTargetIds;
  const pricingRepairTargetIds = recommendedTargets.pricingRepairTargetIds;
  const liveCloudTargetIds = targets.filter(target => targetIsSelectableModel(target) && isCloudModelTarget(target)).map(target => target.id);
  const recommendedPack = recommendedComparisonPackId(packs);
  const localRuntimeCheck = dashboardLocalRuntimeCheck(checks);
  const cloudSetupAdapterId = usePreferredCloudSetupAdapterId(adapters, checks);
  function openBenchmarkStep(check: DoctorCheck) {
    if (check.command.startsWith('Runs > Local + cloud') && recommendedTargetIds.length >= 2) {
      openRunBuilder(localCloudRunBuilderIntent(recommendedTargetIds, recommendedPack));
      setMessage(`Run Builder ready for local/cloud comparison: ${recommendedTargetIds.length} target(s), ${benchmarkPackLabel(recommendedPack)}, 3 repetitions, 1 warmup, ${formatCost(defaultComparisonMaxCostUsd)} cap`);
      return;
    }
    if (check.command.startsWith('Runs > Local + cloud') && pricingRepairTargetIds.length) {
      openTargetRepair({ targetIds: pricingRepairTargetIds, code: 'pricing_assumption' });
      setMessage(`Add input/output pricing before running a capped local/cloud comparison: ${previewList(pricingRepairTargetIds)}`);
      return;
    }
    const repairSide = readinessRepairSide(check);
    if (repairSide) {
      const targetIds = failedReadinessRepairTargetIds(targets, repairSide);
      if (targetIds.length) {
        openTargetRepair({ targetIds, code: readinessRepairCode(targets, repairSide) });
        setMessage(`Repairing failed ${repairSide} target: ${previewList(targetIds)}`);
        return;
      }
    }
    if (check.command.startsWith('Settings') && localRuntimeCheck.check.status === 'ok') {
      openTargetSetup({ code: 'local_runtime_detect', benchmarkPackId: recommendedPack, targetIds: setupCloudTargetIds });
      const comparisonNote = setupCloudTargetIds.length ? ` to compare with ${previewList(setupCloudTargetIds)}` : '';
      setMessage(`Detecting local runtimes for the next benchmark step${comparisonNote}`);
      return;
    }
    if (check.command.startsWith('Targets')) {
      openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId: recommendedPack, targetIds: setupLocalTargetIds });
      const comparisonNote = setupLocalTargetIds.length ? ` with ${previewList(setupLocalTargetIds)} selected for comparison` : '';
      setMessage(`Preparing cloud target setup for the next benchmark step${comparisonNote}`);
      return;
    }
    setPage(nextBenchmarkStepPage(check));
  }
  async function installLocalModelTools() {
    const installPython = checks.some(check => check.id === 'python3' && check.status !== 'ok');
    const installHf = checks.some(check => check.id === 'hf' && check.status !== 'ok');
    const installLlama = checks.some(check => check.id === 'llama-server' && check.status !== 'ok');
    if (!installPython && !installHf && !installLlama) {
      setMessage('Local model tools already look ready');
      return;
    }
    setActionBusy('install-local-tools');
    setBusy(true);
    try {
      const result = await installHuggingFaceTools(installHf, installLlama, installPython);
      setActionLog(result.log || `Install status: ${result.status}`);
      await onRefresh();
      setMessage(`Local tool install ${result.status}; Doctor refreshed`);
    } catch (error) {
      setActionLog(String(error));
      setMessage(String(error));
    } finally {
      setActionBusy('');
      setBusy(false);
    }
  }
  async function copyDoctorCommand(check: DoctorCheck) {
    await navigator.clipboard.writeText(check.command);
    setMessage(`Copied ${check.label} command/path`);
  }
  async function validateDoctorCloudTargets() {
    if (!liveCloudTargetIds.length) {
      openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId: recommendedPack, targetIds: setupLocalTargetIds });
      setMessage('Add a cloud target before running live cloud validation');
      return;
    }
    setActionBusy('validate-cloud-targets');
    setBusy(true);
    try {
      setMessage(`Validating ${liveCloudTargetIds.length} cloud target(s) with live provider probes`);
      const validationResults = await Promise.all(liveCloudTargetIds.map(id => validateTarget(id)));
      await onRefresh();
      const blockers = validationResults.filter(result => result.status === 'error');
      if (blockers.length) {
        openTargetRepair({ targetIds: blockers.map(blocker => blocker.targetId), code: validationRepairCode(blockers[0]) });
        setMessage(`Cloud validation found ${formatValidationCodeCounts(blockers)}. Fix the affected target before running live comparisons.`);
        return;
      }
      const warnings = validationResults.filter(result => result.status !== 'ok');
      if (warnings.length) {
        setMessage(`Cloud validation finished with warnings: ${formatValidationCodeCounts(warnings)}`);
        return;
      }
      setMessage(`Validated ${validationResults.length} cloud target(s); Doctor refreshed with live cloud evidence.`);
    } catch (error) {
      setMessage(`Cloud validation failed: ${String(error)}`);
    } finally {
      setActionBusy('');
      setBusy(false);
    }
  }
  return <section>
    <div className="section-head"><h1>Doctor</h1><div className="actions">{installableLocalMissing ? <button disabled={Boolean(actionBusy)} onClick={() => installLocalModelTools().catch(error => setMessage(String(error)))}><Wrench size={16} />Install local tools</button> : null}<button disabled={Boolean(actionBusy)} onClick={() => onRefresh()}><RefreshCw size={16} />Run</button></div></div>
    <div className="doctor-summary">
      <Card title="Errors" value={errors} note="must fix" />
      <Card title="Warnings" value={warnings} note="optional or workflow-specific" />
      <Card title="Ready" value={ok} note={`${checks.length} checks`} />
    </div>
    {actionLog ? <pre className="setup-log">{actionLog}</pre> : null}
    <BenchmarkNextStepPanel checks={checks} openBenchmarkStep={openBenchmarkStep} />
    <DiagnosticsPanel diagnostics={diagnostics} />
    <table>
      <thead><tr><th>Category</th><th>Check</th><th>Need</th><th>Status</th><th>Detail</th><th>Fix</th><th>Action</th><th>Command</th></tr></thead>
      <tbody>{checks.map(c => <tr key={c.id}>
        <td>{c.category}</td>
        <td>{c.label}</td>
        <td><span className="mini-tag">{c.importance}</span></td>
        <td><span className={`pill ${c.status}`}>{c.status}</span></td>
        <td>{c.detail}</td>
        <td className="doctor-remediation">{c.remediation || '-'}</td>
        <td>{doctorAction(c, targets, cloudSetupAdapterId, recommendedPack, actionBusy, installLocalModelTools, validateDoctorCloudTargets, setPage, setMessage, openBenchmarkStep, openTargetRepair, openTargetSetup, openHuggingFaceLocalSetup)}</td>
        <td className="doctor-command">{c.command ? <div className="doctor-command-cell"><code>{c.command}</code><button title="Copy command or path" onClick={() => copyDoctorCommand(c).catch(error => setMessage(String(error)))}><Copy size={14} /></button></div> : '-'}</td>
      </tr>)}</tbody>
    </table>
  </section>;
}

function BenchmarkNextStepPanel({
  checks,
  openBenchmarkStep,
  primaryDisabled = false,
  primaryTitle,
  primaryLabel,
  primaryIcon,
}: {
  checks: DoctorCheck[];
  openBenchmarkStep: (check: DoctorCheck) => void;
  primaryDisabled?: boolean;
  primaryTitle?: string;
  primaryLabel?: string;
  primaryIcon?: ReactNode;
}) {
  const nextStep = dashboardCheck(checks, 'benchmark-next-step', 'Next benchmark step', 'warn', 'Add one local model target and one cloud model target');
  const stepChecks = [
    dashboardCheck(checks, 'benchmark-target-local', 'Local', 'warn', 'Add local target'),
    dashboardCheck(checks, 'benchmark-target-cloud', 'Cloud', 'warn', 'Add cloud target'),
    dashboardCheck(checks, 'benchmark-local-cloud-results', 'Comparison', 'warn', 'Run local + cloud'),
    dashboardCheck(checks, 'benchmark-local-cloud-evidence', 'Evidence', 'warn', 'Quality pack + cost'),
  ];
  return <div className={`preflight-box next-step-box ${nextStep.status}`}>
    <div className="panel-head">
      <h2>{nextStep.label}</h2>
      <span className={`pill ${nextStep.status}`}>{nextStep.status}</span>
    </div>
    <p>{nextStep.detail}</p>
    {nextStep.remediation ? <p>{nextStep.remediation}</p> : null}
    <div className="actions">
      <button disabled={primaryDisabled} title={primaryTitle} onClick={() => openBenchmarkStep(nextStep)}>{primaryIcon ?? nextBenchmarkStepIcon(nextStep)}{primaryLabel ?? nextBenchmarkStepLabel(nextStep)}</button>
    </div>
    <div className="next-step-track">
      {stepChecks.map(check => <span key={check.id} className={check.status}><strong>{check.label}</strong>{check.status}</span>)}
    </div>
  </div>;
}

function dashboardBenchmarkStepLabel(check: DoctorCheck) {
  return check.command ? nextBenchmarkStepLabel(check) : 'Open readiness checks';
}

function dashboardBenchmarkStepIcon(check: DoctorCheck) {
  return check.command ? nextBenchmarkStepIcon(check) : <ShieldCheck size={14} />;
}

function huggingFaceLocalModelSetupMessage(targets: Target[], setupCloudTargetIds: string[], benchmarkPackId: string) {
  const setupTargets = setupCloudTargetIds
    .map(id => targets.find(target => target.id === id))
    .filter((target): target is Target => Boolean(target));
  const cloudTargets = setupTargets.filter(isCloudModelTarget);
  const pricedCloudTargets = cloudTargets.filter(targetHasInputOutputPricing);
  const unpricedCloudTargets = cloudTargets.filter(target => !targetHasInputOutputPricing(target));
  if (pricedCloudTargets.length) {
    const unpricedNote = unpricedCloudTargets.length
      ? ` Add pricing for ${previewList(unpricedCloudTargets.map(target => target.name))} before including them.`
      : '';
    return `Open Hugging Face Local Model. Start after download will compare the new local target with ${previewList(pricedCloudTargets.map(target => target.name))} using ${benchmarkPackLabel(benchmarkPackId)}.${unpricedNote}`;
  }
  if (unpricedCloudTargets.length) {
    return `Open Hugging Face Local Model. Add input/output pricing for ${previewList(unpricedCloudTargets.map(target => target.name))} before automatic local/cloud comparison. Until then, Start after download can still create the local target and run ${benchmarkPackLabel(benchmarkPackId)} locally.`;
  }
  return 'Open Hugging Face Local Model to search, download, start, and benchmark a local GGUF model. Add a priced cloud target when you want the same benchmark compared against cloud.';
}

function nextBenchmarkStepPage(check: DoctorCheck): Page {
  if (check.command.startsWith('Settings')) {
    return 'settings';
  }
  if (check.command.startsWith('Targets')) {
    return 'targets';
  }
  if (check.command.startsWith('Results')) {
    return 'results';
  }
  return 'runs';
}

function nextBenchmarkStepLabel(check: DoctorCheck) {
  if (check.command.startsWith('Settings')) {
    return 'Set up local';
  }
  if (check.command.startsWith('Runs > Local + cloud')) {
    return 'Run comparison';
  }
  if (check.command.startsWith('Targets > Repair local')) {
    return 'Repair local';
  }
  if (check.command.startsWith('Targets > Repair cloud')) {
    return 'Repair cloud';
  }
  if (check.command.startsWith('Targets')) {
    return 'Set up cloud';
  }
  if (check.command.startsWith('Results')) {
    return 'Open results';
  }
  return 'Open runs';
}

function nextBenchmarkStepIcon(check: DoctorCheck) {
  if (check.command.startsWith('Targets > Repair')) {
    return <Wrench size={14} />;
  }
  if (check.command.startsWith('Settings') || check.command.startsWith('Targets')) {
    return <Settings size={14} />;
  }
  if (check.command.startsWith('Results')) {
    return <ClipboardCheck size={14} />;
  }
  return <Play size={14} />;
}

type ReadinessRepairSide = 'local' | 'cloud';

function readinessRepairSide(check: DoctorCheck): ReadinessRepairSide | null {
  if (check.command.startsWith('Targets > Repair local')) {
    return 'local';
  }
  if (check.command.startsWith('Targets > Repair cloud')) {
    return 'cloud';
  }
  return null;
}

function failedReadinessRepairTargetIds(targets: Target[], side: ReadinessRepairSide) {
  return targets
    .filter(target => target.enabled !== false && target.validationStatus === 'error')
    .filter(target => side === 'local' ? isLocalModelTarget(target) : isCloudModelTarget(target))
    .map(target => target.id);
}

function readinessRepairCode(targets: Target[], side: ReadinessRepairSide) {
  const target = targets.find(candidate => failedReadinessRepairTargetIds([candidate], side).length > 0);
  const validation = target ? targetValidationFromTarget(target) : undefined;
  return validation ? validationRepairCode(validation) : 'needs_review';
}

function openReadinessTargetRepair(targets: Target[], side: ReadinessRepairSide, openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void, setMessage: (message: string) => void) {
  const targetIds = failedReadinessRepairTargetIds(targets, side);
  if (!targetIds.length) {
    setMessage(`No failed ${side} target found to repair`);
    return;
  }
  openTargetRepair({ targetIds, code: readinessRepairCode(targets, side) });
  setMessage(`Repairing failed ${side} target: ${previewList(targetIds)}`);
}

function DiagnosticsPanel({ diagnostics }: { diagnostics: DiagnosticRecord[] }) {
  return <div className="panel compact">
    <div className="panel-head"><h2>Diagnostics</h2><span className="muted">{diagnostics.length ? `${diagnostics.length} recent` : 'no events'}</span></div>
    {diagnostics.length ? <>
      <table className="compact-table">
        <thead><tr><th>Time</th><th>Level</th><th>Kind</th><th>Message</th></tr></thead>
        <tbody>{diagnostics.slice(0, 8).map(event => <tr key={event.id}>
          <td>{formatDateTime(event.createdAt)}</td>
          <td><span className={`pill ${event.level === 'error' ? 'error' : event.level === 'warn' ? 'warn' : 'ok'}`}>{event.level}</span></td>
          <td>{event.kind}</td>
          <td className="diagnostic-detail">{event.message}{event.detail ? <div className="muted">{event.detail}</div> : null}</td>
        </tr>)}</tbody>
      </table>
      <p className="muted">Diagnostics are stored as redacted JSONL at {diagnostics[0].logPath}.</p>
    </> : <p className="muted">No crash or frontend error diagnostics recorded.</p>}
  </div>;
}

function isLocalModelToolCheck(check: DoctorCheck) {
  return check.id === 'python3' || check.id === 'hf' || check.id === 'llama-server';
}

function cloudKeyDoctorAdapterId(check: DoctorCheck) {
  return check.id.startsWith('cloud-key-') ? check.id.slice('cloud-key-'.length) : '';
}

function doctorAction(check: DoctorCheck, targets: Target[], cloudSetupAdapterId: string, benchmarkPackId: string, actionBusy: string, installLocalModelTools: () => Promise<void>, validateCloudTargets: () => Promise<void>, setPage: (page: Page) => void, setMessage: (message: string) => void, openBenchmarkStep: (check: DoctorCheck) => void, openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void, openTargetSetup: (intent: Omit<TargetSetupIntent, 'nonce'>) => void, openHuggingFaceLocalSetup: (intent?: Omit<HuggingFaceLocalSetupIntent, 'nonce'>) => void) {
  const recommendedTargets = dashboardLocalCloudComparisonTargets(targets);
  const liveCloudTargetCount = targets.filter(target => targetIsSelectableModel(target) && isCloudModelTarget(target)).length;
  if (isLocalModelToolCheck(check) && check.status !== 'ok') {
    return <button disabled={Boolean(actionBusy)} onClick={() => installLocalModelTools().catch(error => setMessage(String(error)))}><Wrench size={14} />Install</button>;
  }
  if (check.id === 'hf-model-storage') {
    return <button onClick={() => {
      openHuggingFaceLocalSetup({ benchmarkPackId, targetIds: recommendedTargets.setupCloudTargetIds });
      setMessage(huggingFaceLocalModelSetupMessage(targets, recommendedTargets.setupCloudTargetIds, benchmarkPackId));
    }}><Settings size={14} />Local</button>;
  }
  if (check.id.startsWith('cloud-key-')) {
    const adapterId = cloudKeyDoctorAdapterId(check);
    return <button onClick={() => {
      openTargetSetup({ adapterId, code: 'missing_key', benchmarkPackId, targetIds: recommendedTargets.setupLocalTargetIds });
    }}><Settings size={14} />Key</button>;
  }
  if (check.id.startsWith('endpoint-')) {
    return <button onClick={() => {
      openTargetSetup({ code: 'local_runtime_detect', benchmarkPackId, targetIds: recommendedTargets.setupCloudTargetIds });
    }}><Search size={14} />Detect</button>;
  }
  if (check.id === 'benchmark-target-local') {
    const repair = check.detail.includes('last validation failed');
    return <button onClick={() => {
      if (repair) {
        openReadinessTargetRepair(targets, 'local', openTargetRepair, setMessage);
      } else {
        openHuggingFaceLocalSetup({ benchmarkPackId, targetIds: recommendedTargets.setupCloudTargetIds });
        setMessage(huggingFaceLocalModelSetupMessage(targets, recommendedTargets.setupCloudTargetIds, benchmarkPackId));
      }
    }}>{repair ? <Wrench size={14} /> : <Settings size={14} />}{repair ? 'Repair' : 'Local'}</button>;
  }
  if (check.id === 'benchmark-target-cloud') {
    const repair = check.detail.includes('last validation failed');
    return <button onClick={() => {
      if (repair) {
        openReadinessTargetRepair(targets, 'cloud', openTargetRepair, setMessage);
      } else {
        openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId, targetIds: recommendedTargets.setupLocalTargetIds });
      }
    }}>{repair ? <Wrench size={14} /> : <Settings size={14} />}{repair ? 'Repair' : 'Cloud'}</button>;
  }
  if (check.id === 'benchmark-local-cloud-compare') {
    return <button onClick={() => openBenchmarkStep(check)}><Play size={14} />Runs</button>;
  }
  if (check.id === 'benchmark-local-cloud-evidence') {
    return <button onClick={() => check.status === 'ok' ? setPage('results') : openBenchmarkStep(check)}><ClipboardCheck size={14} />{check.status === 'ok' ? 'Results' : 'Runs'}</button>;
  }
  if (check.id === 'benchmark-next-step') {
    return <button onClick={() => openBenchmarkStep(check)}>{nextBenchmarkStepIcon(check)}{nextBenchmarkStepLabel(check)}</button>;
  }
  if (check.id === 'benchmark-packs' || check.id === 'benchmark-pack-diagnostics') {
    return <button onClick={() => setPage('benchmarks')}><FlaskConical size={14} />Packs</button>;
  }
  if (check.id === 'product-live-cloud') {
    return <button disabled={Boolean(actionBusy)} title={liveCloudTargetCount ? `Validate ${liveCloudTargetCount} cloud target(s)` : 'Add a cloud target before validating live provider access'} onClick={() => validateCloudTargets().catch(error => setMessage(String(error)))}>{liveCloudTargetCount ? <ShieldCheck size={14} /> : <Boxes size={14} />}{liveCloudTargetCount ? 'Validate' : 'Cloud'}</button>;
  }
  if (check.id === 'docker' || check.id === 'colima') {
    return <button onClick={() => setPage('runs')}><ShieldCheck size={14} />Runs</button>;
  }
  return <span className="muted">-</span>;
}

function ArtifactContent({ artifact, text }: { artifact: Artifact; text: string }) {
  if (artifactViewClass(artifact) === 'diff') {
    return <pre className="artifact-view diff">{text.split('\n').map((line, index) => <span key={`${index}-${line.slice(0, 8)}`} className={diffLineClass(line)}>{line || ' '}</span>)}</pre>;
  }
  return <pre className={`artifact-view ${artifactViewClass(artifact)}`}>{text}</pre>;
}

function compareArtifacts(a: Artifact, b: Artifact) {
  return artifactPriority(a) - artifactPriority(b) || artifactDisplayName(a).localeCompare(artifactDisplayName(b));
}

function pickPreferredArtifact(artifacts: Artifact[]) {
  return artifacts.find(artifact => artifact.kind === 'response')
    ?? artifacts.find(artifact => artifact.kind === 'raw_response' || artifact.kind === 'response_json')
    ?? artifacts.find(artifact => artifact.kind === 'git_diff')
    ?? artifacts.find(artifact => artifact.kind === 'stdout')
    ?? artifacts[0];
}

function artifactPriority(artifact: Artifact) {
  const order: Record<string, number> = {
    prompt: 10,
    response: 20,
    response_json: 25,
    raw_response: 30,
    result_json: 40,
    stdout: 50,
    stderr: 60,
    git_diff: 70,
    worker_jsonl: 80,
  };
  return order[artifact.kind] ?? 100;
}

function artifactLabel(artifact: Artifact) {
  const labels: Record<string, string> = {
    prompt: 'Prompt',
    response: 'Response',
    response_json: 'Response JSON',
    raw_response: 'Raw Provider Response',
    result_json: 'Scoring Result',
    stdout: 'Stdout',
    stderr: 'Stderr',
    git_diff: 'Git Diff',
    worker_jsonl: 'Worker Events',
  };
  return labels[artifact.kind] ?? artifact.kind.replace(/_/g, ' ');
}

function artifactDisplayName(artifact: Artifact) {
  return artifact.path.split('/').pop() || artifact.path;
}

function artifactViewClass(artifact: Artifact) {
  const name = artifactDisplayName(artifact).toLowerCase();
  if (artifact.kind === 'git_diff' || name.endsWith('.patch') || name.endsWith('.diff')) {
    return 'diff';
  }
  if (artifact.kind.includes('json') || name.endsWith('.json') || name.endsWith('.jsonl')) {
    return 'json';
  }
  return 'log';
}

function formatArtifactText(artifact: Artifact, text: string) {
  if (artifactViewClass(artifact) !== 'json') {
    return text;
  }
  const trimmed = text.trim();
  if (!trimmed) {
    return text;
  }
  if (artifactDisplayName(artifact).toLowerCase().endsWith('.jsonl')) {
    return trimmed.split('\n').map(line => {
      try {
        return JSON.stringify(JSON.parse(line), null, 2);
      } catch {
        return line;
      }
    }).join('\n');
  }
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return text;
  }
}

function diffLineClass(line: string) {
  if (line.startsWith('+') && !line.startsWith('+++')) {
    return 'diff-add';
  }
  if (line.startsWith('-') && !line.startsWith('---')) {
    return 'diff-remove';
  }
  if (line.startsWith('@@')) {
    return 'diff-hunk';
  }
  return '';
}

function shortHash(value: string) {
  return value.length > 12 ? `${value.slice(0, 12)}...` : value;
}

interface ComparisonRow {
  key: string;
  groupId: string;
  packId: string;
  targetId: string;
  providers: string;
  runs: number;
  passed: number;
  passRate: number;
  avgScore: number | null;
  scoreStdDev: number | null;
  medianScore: number | null;
  minScore: number | null;
  maxScore: number | null;
  avgTimeMs: number | null;
  p95TimeMs: number | null;
  timeStdDevMs: number | null;
  medianTimeMs: number | null;
  minTimeMs: number | null;
  maxTimeMs: number | null;
  avgProviderTimeToFirstByteMs: number | null;
  avgProviderTimeToFirstTokenMs: number | null;
  avgProviderRequestTotalMs: number | null;
  avgTokens: number | null;
  avgReasoningTokens: number | null;
  avgOutputTokensPerSecond: number | null;
  avgAttempts: number | null;
  httpStatuses: string;
  providerModels: string;
  providerModelSources: string;
  providerModelCount: number;
  missingProviderModelRuns: number;
  configuredProviderModelRuns: number;
  generationSettingCounts: Map<string, number>;
  generationSettings: string;
  generationSettingCount: number;
  finishReasons: string;
  pricingAssumptions: string;
  pricingAssumptionCount: number;
  avgCostUsd: number | null;
  firstRunId: string;
  latestStarted: string;
}

interface RunGroupTrendRow {
  key: string;
  packId: string;
  targetId: string;
  current: ComparisonRow;
  previous: ComparisonRow;
  passRateDelta: number;
  avgScoreDelta: number | null;
  p95TimeDeltaMs: number | null;
  avgCostDeltaUsd: number | null;
  signalLevel: 'ok' | 'warn';
  signal: string;
}

interface TaskComparisonRow {
  key: string;
  groupId: string;
  packId: string;
  taskId: string;
  targetId: string;
  runs: number;
  passed: number;
  passRate: number;
  avgScore: number | null;
  scoreStdDev: number | null;
  medianScore: number | null;
  minScore: number | null;
  maxScore: number | null;
  avgTimeMs: number | null;
  p95TimeMs: number | null;
  medianTimeMs: number | null;
  minTimeMs: number | null;
  maxTimeMs: number | null;
  avgProviderTimeToFirstTokenMs: number | null;
  avgTokens: number | null;
  avgOutputTokensPerSecond: number | null;
  httpStatuses: string;
  errorCodes: string;
  avgCostUsd: number | null;
  firstRunId: string;
  latestStarted: string;
}

interface TaskTargetMatrix {
  targets: string[];
  rows: TaskTargetMatrixRow[];
}

interface TaskTargetMatrixRow {
  key: string;
  groupId: string;
  packId: string;
  taskId: string;
  cells: Record<string, TaskTargetMatrixCell>;
}

interface TaskTargetMatrixCell {
  runs: number;
  passed: number;
  passRate: number;
  avgScore: number | null;
  p95TimeMs: number | null;
  errorCodes: string;
  firstRunId: string;
}

interface TargetRankingRow {
  targetId: string;
  providers: string;
  runs: number;
  passed: number;
  passRate: number;
  totalTaskWeight: number;
  weightedPassRate: number | null;
  weightedAvgScore: number | null;
  passRateCiLow: number | null;
  passRateCiHigh: number | null;
  avgScore: number | null;
  scoreStdDev: number | null;
  medianScore: number | null;
  minScore: number | null;
  maxScore: number | null;
  p95TimeMs: number | null;
  medianTimeMs: number | null;
  minTimeMs: number | null;
  maxTimeMs: number | null;
  avgCostUsd: number | null;
  costedRuns: number;
  pricingAssumptionRuns: number;
  pricingAssumptionIds: string[];
  avgOutputTokensPerSecond: number | null;
  groups: number;
  packs: number;
  tasks: number;
  groupIds: string[];
  packIds: string[];
  taskIds: string[];
  packTaskSlots: string[];
  errorCodes: string;
  firstRunId: string;
  latestStarted: string;
}

interface ResultCoverageIssue {
  targetId: string;
  missingPackTaskSlots: string[];
  missingPacks: string[];
  missingTasks: string[];
}

interface ErrorRecoveryRow {
  key: string;
  code: string;
  count: number;
  packIds: string[];
  targetIds: string[];
  taskIds: string[];
  httpStatuses: string;
  retryable: boolean;
  recoveryHint: string;
  exampleDetail: string;
}

interface ErrorRecoveryTargetAvailability {
  selectableTargetIds: string[];
  unavailableTargetIds: string[];
  missingTargetIds: string[];
}

interface ModelIdentityWarning {
  key: string;
  issue: 'provider_model_missing' | 'provider_model_inconsistent' | 'provider_model_configured_fallback';
  groupId: string;
  packId: string;
  targetId: string;
  runs: number;
  missingProviderModelRuns: number;
  configuredProviderModelRuns: number;
  providerModels: string;
  providerModelSources: string;
  note: string;
  firstRunId: string;
}

interface GenerationSettingWarning {
  key: string;
  issue: 'generation_settings_mixed_target' | 'generation_settings_mixed_scope';
  groupId: string;
  packId: string;
  targetId: string;
  runs: number;
  generationSettings: string;
  note: string;
  firstRunId: string;
}

interface PackCalibrationIssue {
  packId: string;
  statuses: string[];
  sampleSizes: number[];
  baselineModels: string[];
  lastReviewed: string[];
  qualityGates: string[];
  missingQualityGates: string[];
  notes: string[];
}

interface PackEvidenceIssue {
  packId: string;
  evidenceProfile: string;
  warnings: string[];
}

type ComparisonEvidenceGrade = 'insufficient' | 'smoke' | 'directional' | 'comparison_ready';
type ModelSelectionDecisionStatus = 'insufficient_evidence' | 'collect_more_evidence' | 'select_recommended_target';

interface ResultEvidenceSummary {
  tone: 'ok' | 'warn';
  grade: ComparisonEvidenceGrade;
  label: string;
  headline: string;
  notes: string[];
  coverageIssues: ResultCoverageIssue[];
  risks: string[];
  minimumNextRun: string;
  nextRunIntent: RunBuilderIntent | null;
  pricingRepairTargetIds: string[];
}

interface DecisionSnapshot {
  decisionStatus: ModelSelectionDecisionStatus;
  selectedTargetId: string | null;
  selectionNote: string;
  recommendedTarget: TargetRankingRow;
  closeContenders: TargetRankingRow[];
  bestOverall: ComparisonRow;
  fastestReliable?: ComparisonRow;
  cheapestReliable?: ComparisonRow;
  throughputLeader?: ComparisonRow;
  weakestTask?: TaskComparisonRow;
  confidenceNote: string;
  coverageNote: string;
  evidenceGrade: ComparisonEvidenceGrade;
  evidenceLabel: string;
  evidenceNote: string;
  packEvidenceIssues: PackEvidenceIssue[];
  packCalibrationIssues: PackCalibrationIssue[];
  calibrationNote: string;
  minimumNextRun: string;
  scoreStabilityNote: string;
}

interface ChartRow {
  label: string;
  value: number;
  valueLabel: string;
  max?: number;
  tone?: string;
}

interface MetricCoverageRow {
  label: string;
  present: number;
  missing: number;
  note: string;
}

function MetricCoveragePanel({ rows, total }: { rows: MetricCoverageRow[]; total: number }) {
  const rowsWithGaps = rows.filter(row => total > 0 && row.missing > 0);
  const headline = total ? `${rows.length - rowsWithGaps.length}/${rows.length} metrics complete in current scope` : 'No results in current scope';
  return <div className={`preflight-box ${rowsWithGaps.length ? 'warn' : 'ok'}`}>
    <strong>Metric Coverage</strong>
    <p>Blank metric cells mean BenchForge did not receive enough source data for that metric; they are not treated as zero.</p>
    <p>{headline}</p>
    {total ? <table className="compact-table metric-coverage-table"><thead><tr><th>Metric</th><th>Present</th><th>Missing</th><th>Notes</th></tr></thead><tbody>{rows.map(row => <tr key={row.label}><td>{row.label}</td><td>{row.present}/{total}</td><td>{row.missing}</td><td>{row.note}</td></tr>)}</tbody></table> : null}
  </div>;
}

function ModelIdentityWarningsPanel({ warnings, onSelectRun, selectedRunId }: { warnings: ModelIdentityWarning[]; onSelectRun: (id: string) => void; selectedRunId: string }) {
  return <div className="preflight-box warn">
    <strong>Model Identity Warnings</strong>
    <p>Confirm the served model before treating these rows as definitive model-selection evidence.</p>
    <table className="compact-table"><thead><tr><th>Issue</th><th>Group</th><th>Pack</th><th>Target</th><th>Runs</th><th>Missing</th><th>Configured</th><th>Reported models</th><th>Sources</th><th>Note</th></tr></thead><tbody>
      {warnings.map(warning => <tr key={warning.key} onClick={() => onSelectRun(warning.firstRunId)} className={warning.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{warning.issue}</td><td>{warning.groupId.slice(0, 8)}</td><td>{warning.packId}</td><td>{warning.targetId}</td><td>{warning.runs}</td><td>{warning.missingProviderModelRuns}</td><td>{warning.configuredProviderModelRuns}</td><td>{warning.providerModels}</td><td>{warning.providerModelSources}</td><td>{warning.note}</td></tr>)}
    </tbody></table>
  </div>;
}

function GenerationSettingWarningsPanel({ warnings, onSelectRun, selectedRunId }: { warnings: GenerationSettingWarning[]; onSelectRun: (id: string) => void; selectedRunId: string }) {
  return <div className="preflight-box warn">
    <strong>Generation Setting Warnings</strong>
    <p>Use one shared temperature, top-p, and seed policy before treating the visible rows as definitive model-selection evidence.</p>
    <table className="compact-table"><thead><tr><th>Issue</th><th>Group</th><th>Pack</th><th>Target</th><th>Runs</th><th>Settings</th><th>Note</th></tr></thead><tbody>
      {warnings.map(warning => <tr key={warning.key} onClick={() => onSelectRun(warning.firstRunId)} className={warning.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{warning.issue}</td><td>{warning.groupId === 'all' ? 'all' : warning.groupId.slice(0, 8)}</td><td>{warning.packId}</td><td>{warning.targetId}</td><td>{warning.runs}</td><td>{warning.generationSettings}</td><td>{warning.note}</td></tr>)}
    </tbody></table>
  </div>;
}

function PackCalibrationWarningsPanel({ issues }: { issues: PackCalibrationIssue[] }) {
  return <div className="preflight-box warn">
    <strong>Pack Calibration Warnings</strong>
    <p>{packCalibrationNote(issues)}</p>
    <table className="compact-table"><thead><tr><th>Pack</th><th>Status</th><th>Sample size</th><th>Baselines</th><th>Reviewed</th><th>Missing gates</th><th>Note</th></tr></thead><tbody>
      {issues.map(issue => <tr key={issue.packId}><td>{issue.packId}</td><td>{previewList(issue.statuses)}</td><td>{formatNumberList(issue.sampleSizes)}</td><td>{previewList(issue.baselineModels)}</td><td>{previewList(issue.lastReviewed)}</td><td>{previewList(issue.missingQualityGates)}</td><td>{previewList(issue.notes, 1)}</td></tr>)}
    </tbody></table>
  </div>;
}

function PackEvidenceWarningsPanel({ issues }: { issues: PackEvidenceIssue[] }) {
  return <div className="preflight-box warn">
    <strong>Pack Evidence Warnings</strong>
    <p>{packEvidenceNote(issues)}</p>
    <table className="compact-table"><thead><tr><th>Pack</th><th>Evidence profile</th><th>Warning</th></tr></thead><tbody>
      {issues.map(issue => <tr key={issue.packId}><td>{issue.packId}</td><td>{formatEvidenceProfile(issue.evidenceProfile)}</td><td>{previewList(issue.warnings, 1)}</td></tr>)}
    </tbody></table>
  </div>;
}

function ResultEvidencePanel({ evidence, onOpenNextRun, onRepairPricingAssumptions }: { evidence: ResultEvidenceSummary; onOpenNextRun?: (intent: RunBuilderIntent) => void; onRepairPricingAssumptions?: (targetIds: string[]) => void }) {
  return <div className={`preflight-box ${evidence.tone}`}>
    <strong>Comparison Evidence: {evidence.label}</strong>
    <p>{evidence.headline}</p>
    {evidence.notes.map(note => <p key={note}>{note}</p>)}
    {evidence.risks.length ? <p>Risks: {evidence.risks.join(', ')}</p> : null}
    <p>Minimum next run: {evidence.minimumNextRun}</p>
    {(evidence.nextRunIntent && onOpenNextRun) || (evidence.pricingRepairTargetIds.length && onRepairPricingAssumptions) ? <div className="row-actions">
      {evidence.nextRunIntent && onOpenNextRun ? <button onClick={() => onOpenNextRun(evidence.nextRunIntent!)}><Play size={14} />Open next run</button> : null}
      {evidence.pricingRepairTargetIds.length && onRepairPricingAssumptions ? <button title={errorCategoryRepairHint('pricing_assumption')} onClick={() => onRepairPricingAssumptions(evidence.pricingRepairTargetIds)}><Wrench size={14} />Repair pricing</button> : null}
    </div> : null}
    {evidence.coverageIssues.length ? <table className="compact-table"><thead><tr><th>Target</th><th>Missing pack/task slots</th><th>Missing packs</th><th>Missing tasks</th></tr></thead><tbody>
      {evidence.coverageIssues.slice(0, 5).map(issue => <tr key={issue.targetId}><td>{issue.targetId}</td><td>{previewList(issue.missingPackTaskSlots)}</td><td>{previewList(issue.missingPacks)}</td><td>{previewList(issue.missingTasks)}</td></tr>)}
    </tbody></table> : null}
  </div>;
}

function DecisionSnapshotPanel({ decision, onSelectRun, selectedRunId }: { decision: DecisionSnapshot; onSelectRun: (id: string) => void; selectedRunId: string }) {
  const rows: Array<{ label: string; row: ComparisonRow | TargetRankingRow; metric: string }> = [{
    label: 'Recommended target',
    row: decision.recommendedTarget,
    metric: `${formatPercent(decision.recommendedTarget.weightedPassRate)} weighted pass, ${formatPercent(decision.recommendedTarget.passRate)} pass across ${decision.recommendedTarget.runs} run(s), 95% CI ${formatPercentRange(decision.recommendedTarget.passRateCiLow, decision.recommendedTarget.passRateCiHigh)}, weighted score ${formatNumber(decision.recommendedTarget.weightedAvgScore)}, score ${formatNumberWithSpread(decision.recommendedTarget.avgScore, decision.recommendedTarget.scoreStdDev)}`,
  }];
  for (const contender of decision.closeContenders) {
    rows.push({
      label: 'Close contender',
      row: contender,
      metric: `matched pass rate and score; 95% CI ${formatPercentRange(contender.passRateCiLow, contender.passRateCiHigh)}, score ${formatNumberWithSpread(contender.avgScore, contender.scoreStdDev)}, p95 ${formatMs(contender.p95TimeMs)}, cost ${formatCost(contender.avgCostUsd)}`,
    });
  }
  rows.push({
    label: 'Best overall',
    row: decision.bestOverall,
    metric: `${formatPercent(decision.bestOverall.passRate)} pass, score ${formatNumber(decision.bestOverall.avgScore)}, p95 ${formatMs(decision.bestOverall.p95TimeMs)}`,
  });
  if (decision.fastestReliable) {
    rows.push({
      label: 'Fastest reliable',
      row: decision.fastestReliable,
      metric: `p95 ${formatMs(decision.fastestReliable.p95TimeMs)}, ${formatPercent(decision.fastestReliable.passRate)} pass`,
    });
  }
  if (decision.cheapestReliable) {
    rows.push({
      label: 'Cheapest reliable',
      row: decision.cheapestReliable,
      metric: `${formatCost(decision.cheapestReliable.avgCostUsd)} avg, ${formatPercent(decision.cheapestReliable.passRate)} pass`,
    });
  }
  if (decision.throughputLeader) {
    rows.push({
      label: 'Highest throughput',
      row: decision.throughputLeader,
      metric: `${formatNumber(decision.throughputLeader.avgOutputTokensPerSecond)} out tok/s, ${formatPercent(decision.throughputLeader.passRate)} pass`,
    });
  }

  return <div className="analysis-grid narrow">
    <div className="panel"><h2>Decision Snapshot</h2><table className="compact-table"><tbody>
      <tr><td>Decision status</td><td><strong>{formatDecisionStatus(decision.decisionStatus)}</strong><div className="muted">{decision.selectedTargetId ? `Selected target: ${decision.selectedTargetId}` : 'No target selected'}</div></td><td>{decision.selectionNote}</td></tr>
      {rows.map(item => <tr key={`${item.label}-${item.row.targetId}`} onClick={() => onSelectRun(item.row.firstRunId)} className={item.row.firstRunId === selectedRunId ? 'selected-row' : ''}><td>{item.label}</td><td><strong>{item.row.targetId}</strong><div className="muted">{decisionRowScope(item.row)}</div></td><td>{item.metric}</td></tr>)}
    </tbody></table><p className="muted"><strong>Evidence:</strong> {decision.evidenceLabel}. {decision.evidenceNote}</p><p className="muted"><strong>Pack evidence:</strong> {packEvidenceNote(decision.packEvidenceIssues)}</p><p className="muted"><strong>Pack calibration:</strong> {decision.calibrationNote}</p><p className="muted">Minimum next run: {decision.minimumNextRun}</p><p className="muted">{decision.confidenceNote}</p><p className="muted">{decision.coverageNote}</p></div>
    <div className="panel"><h2>Risk Signal</h2>{decision.weakestTask ? <table className="compact-table"><tbody><tr onClick={() => onSelectRun(decision.weakestTask!.firstRunId)} className={decision.weakestTask.firstRunId === selectedRunId ? 'selected-row' : ''}><td>Weakest task</td><td><strong>{decision.weakestTask.taskId}</strong><div className="muted">{decision.weakestTask.targetId}</div></td><td>{decision.weakestTask.passed}/{decision.weakestTask.runs} passed, p95 {formatMs(decision.weakestTask.p95TimeMs)}</td></tr></tbody></table> : <p className="muted">No task-level risk signal in scope.</p>}<p className="muted">{decision.scoreStabilityNote}</p><p className="muted">Ranking favors pass rate, score, and score stability before speed, cost, and throughput.</p></div>
  </div>;
}

function decisionRowScope(row: ComparisonRow | TargetRankingRow) {
  if ('packId' in row) {
    return `${row.packId} / ${row.groupId.slice(0, 8)}`;
  }
  return `${row.packs} pack(s), ${row.tasks} task(s), ${row.groups} group(s)`;
}

function ChartPanel({ title, rows, empty = 'No data' }: { title: string; rows: ChartRow[]; empty?: string }) {
  const max = Math.max(...rows.map(row => row.max ?? row.value), 0);
  return <div className="panel chart-panel"><h2>{title}</h2>{rows.length ? <div className="bar-list">{rows.map(row => {
    const denominator = row.max ?? max;
    const width = denominator > 0 ? Math.max(2, Math.min(100, (row.value / denominator) * 100)) : 0;
    return <div className="bar-row" key={row.label}>
      <div className="bar-label"><span>{row.label}</span><strong>{row.valueLabel}</strong></div>
      <div className="bar-track"><div className={`bar-fill ${row.tone ?? ''}`} style={{ width: `${width}%` }} /></div>
    </div>;
  })}</div> : <p className="muted">{empty}</p>}</div>;
}

function DistributionSummaryPanel({ targets, tasks, onSelectRun, selectedRunId }: { targets: TargetRankingRow[]; tasks: TaskComparisonRow[]; onSelectRun: (id: string) => void; selectedRunId: string }) {
  return <div className="panel">
    <h2>Distribution Summary</h2>
    <p className="muted">Median/min/max values expose outliers across the current result scope.</p>
    <table className="compact-table distribution-table"><thead><tr><th>Target</th><th>Runs</th><th>Score med/min/max</th><th>Wall med/min/max</th><th>P95 wall</th></tr></thead><tbody>
      {targets.map(row => <tr key={row.targetId} onClick={() => onSelectRun(row.firstRunId)} className={row.firstRunId === selectedRunId ? 'selected-row' : ''}><td><strong>{row.targetId}</strong><div className="muted">{row.packs} pack(s), {row.tasks} task(s)</div></td><td>{row.runs}</td><td>{formatNumberDistribution(row.medianScore, row.minScore, row.maxScore)}</td><td>{formatMsDistribution(row.medianTimeMs, row.minTimeMs, row.maxTimeMs)}</td><td>{formatMs(row.p95TimeMs)}</td></tr>)}
    </tbody></table>
    {tasks.length ? <>
      <h2>Weakest Task Distributions</h2>
      <table className="compact-table distribution-table"><thead><tr><th>Task</th><th>Target</th><th>Runs</th><th>Pass rate</th><th>Score med/min/max</th><th>Wall med/min/max</th></tr></thead><tbody>
        {tasks.slice(0, 8).map(row => <tr key={row.key} onClick={() => onSelectRun(row.firstRunId)} className={row.firstRunId === selectedRunId ? 'selected-row' : ''}><td><strong>{row.taskId}</strong><div className="muted">{row.packId} / {row.groupId.slice(0, 8)}</div></td><td>{row.targetId}</td><td>{row.runs}</td><td>{row.passed}/{row.runs} ({formatPercent(row.passRate)})</td><td>{formatNumberDistribution(row.medianScore, row.minScore, row.maxScore)}</td><td>{formatMsDistribution(row.medianTimeMs, row.minTimeMs, row.maxTimeMs)}</td></tr>)}
      </tbody></table>
    </> : null}
  </div>;
}

function buildComparisonRows(results: RunResult[], adapterById: Map<string, Adapter>): ComparisonRow[] {
  const groups = new Map<string, {
    key: string;
    groupId: string;
    packId: string;
    targetId: string;
    providerCounts: Map<string, number>;
    runs: number;
    passed: number;
    scoreSum: number;
    scored: number;
    timeSum: number;
    timed: number;
    providerTimeToFirstByteSum: number;
    providerTimeToFirstByteCount: number;
    providerTimeToFirstTokenSum: number;
    providerTimeToFirstTokenCount: number;
    providerRequestTotalSum: number;
    providerRequestTotalCount: number;
    tokenSum: number;
    tokenized: number;
    reasoningTokenSum: number;
    reasoningTokenized: number;
    throughputSum: number;
    throughputMeasured: number;
    attemptSum: number;
    attempted: number;
    httpStatusCounts: Map<number, number>;
    providerModelCounts: Map<string, number>;
    providerModelSourceCounts: Map<string, number>;
    generationSettingCounts: Map<string, number>;
    finishReasonCounts: Map<string, number>;
    pricingAssumptionCounts: Map<string, number>;
    costSum: number;
    costed: number;
    scoreValues: number[];
    timeValues: number[];
    tokenValues: number[];
    costValues: number[];
    firstRunId: string;
    latestStarted: string;
  }>();
  for (const result of results) {
    const groupId = resultGroupId(result);
    const packId = resultPackId(result);
    const targetId = resultTargetId(result);
    const key = `${groupId}|${packId}|${targetId}`;
    const row = groups.get(key) ?? {
      key,
      groupId,
      packId,
      targetId,
      providerCounts: new Map<string, number>(),
      runs: 0,
      passed: 0,
      scoreSum: 0,
      scored: 0,
      timeSum: 0,
      timed: 0,
      providerTimeToFirstByteSum: 0,
      providerTimeToFirstByteCount: 0,
      providerTimeToFirstTokenSum: 0,
      providerTimeToFirstTokenCount: 0,
      providerRequestTotalSum: 0,
      providerRequestTotalCount: 0,
      tokenSum: 0,
      tokenized: 0,
      reasoningTokenSum: 0,
      reasoningTokenized: 0,
      throughputSum: 0,
      throughputMeasured: 0,
      attemptSum: 0,
      attempted: 0,
      httpStatusCounts: new Map<number, number>(),
      providerModelCounts: new Map<string, number>(),
      providerModelSourceCounts: new Map<string, number>(),
      generationSettingCounts: new Map<string, number>(),
      finishReasonCounts: new Map<string, number>(),
      pricingAssumptionCounts: new Map<string, number>(),
      costSum: 0,
      costed: 0,
      scoreValues: [] as number[],
      timeValues: [] as number[],
      tokenValues: [] as number[],
      costValues: [] as number[],
      firstRunId: result.id,
      latestStarted: result.started_at ?? '',
    };
    row.runs += 1;
    const provider = resultProviderLabel(result, adapterById);
    row.providerCounts.set(provider, (row.providerCounts.get(provider) ?? 0) + 1);
    if (result.status === 'passed') {
      row.passed += 1;
    }
    if (typeof result.score === 'number') {
      row.scoreSum += result.score;
      row.scored += 1;
      row.scoreValues.push(result.score);
    }
    const wallTime = resultWallTimeMs(result);
    if (typeof wallTime === 'number') {
      row.timeSum += wallTime;
      row.timed += 1;
      row.timeValues.push(wallTime);
    }
    if (typeof result.provider_time_to_first_byte_ms === 'number') {
      row.providerTimeToFirstByteSum += result.provider_time_to_first_byte_ms;
      row.providerTimeToFirstByteCount += 1;
    }
    if (typeof result.provider_time_to_first_token_ms === 'number') {
      row.providerTimeToFirstTokenSum += result.provider_time_to_first_token_ms;
      row.providerTimeToFirstTokenCount += 1;
    }
    if (typeof result.provider_request_total_ms === 'number') {
      row.providerRequestTotalSum += result.provider_request_total_ms;
      row.providerRequestTotalCount += 1;
    }
    const tokens = totalTokens(result);
    if (tokens != null) {
      row.tokenSum += tokens;
      row.tokenized += 1;
      row.tokenValues.push(tokens);
    }
    if (typeof result.reasoning_tokens === 'number') {
      row.reasoningTokenSum += result.reasoning_tokens;
      row.reasoningTokenized += 1;
    }
    if (typeof result.output_tokens_per_second === 'number') {
      row.throughputSum += result.output_tokens_per_second;
      row.throughputMeasured += 1;
    }
    if (typeof result.provider_attempts === 'number') {
      row.attemptSum += result.provider_attempts;
      row.attempted += 1;
    }
    if (typeof result.http_status === 'number' && Number.isFinite(result.http_status)) {
      const status = Math.round(result.http_status);
      if (status >= 100 && status <= 599) {
        row.httpStatusCounts.set(status, (row.httpStatusCounts.get(status) ?? 0) + 1);
      }
    }
    const providerModel = result.provider_model?.trim();
    if (providerModel) {
      row.providerModelCounts.set(providerModel, (row.providerModelCounts.get(providerModel) ?? 0) + 1);
    }
    const providerModelSource = result.provider_model_source?.trim();
    if (providerModelSource) {
      row.providerModelSourceCounts.set(providerModelSource, (row.providerModelSourceCounts.get(providerModelSource) ?? 0) + 1);
    }
    const generationSetting = resultGenerationSamplingFingerprint(result);
    row.generationSettingCounts.set(generationSetting, (row.generationSettingCounts.get(generationSetting) ?? 0) + 1);
    if (result.finish_reason) {
      row.finishReasonCounts.set(result.finish_reason, (row.finishReasonCounts.get(result.finish_reason) ?? 0) + 1);
    }
    const pricingAssumption = result.pricing_assumption?.trim();
    if (pricingAssumption) {
      row.pricingAssumptionCounts.set(pricingAssumption, (row.pricingAssumptionCounts.get(pricingAssumption) ?? 0) + 1);
    }
    const costUsd = resultCostUsdForCoverage(result);
    if (costUsd != null) {
      row.costSum += costUsd;
      row.costed += 1;
      row.costValues.push(costUsd);
    }
    if ((result.started_at ?? '') > row.latestStarted) {
      row.latestStarted = result.started_at ?? '';
      row.firstRunId = result.id;
    }
    groups.set(key, row);
  }
  return Array.from(groups.values())
    .map(row => ({
      key: row.key,
      groupId: row.groupId,
      packId: row.packId,
      targetId: row.targetId,
      providers: formatTextCounts(row.providerCounts),
      runs: row.runs,
      passed: row.passed,
      passRate: row.runs ? row.passed / row.runs : 0,
      avgScore: row.scored ? row.scoreSum / row.scored : null,
      scoreStdDev: stdDev(row.scoreValues),
      medianScore: median(row.scoreValues),
      minScore: minValue(row.scoreValues),
      maxScore: maxValue(row.scoreValues),
      avgTimeMs: row.timed ? row.timeSum / row.timed : null,
      p95TimeMs: percentile(row.timeValues, 0.95),
      timeStdDevMs: stdDev(row.timeValues),
      medianTimeMs: median(row.timeValues),
      minTimeMs: minValue(row.timeValues),
      maxTimeMs: maxValue(row.timeValues),
      avgProviderTimeToFirstByteMs: row.providerTimeToFirstByteCount ? row.providerTimeToFirstByteSum / row.providerTimeToFirstByteCount : null,
      avgProviderTimeToFirstTokenMs: row.providerTimeToFirstTokenCount ? row.providerTimeToFirstTokenSum / row.providerTimeToFirstTokenCount : null,
      avgProviderRequestTotalMs: row.providerRequestTotalCount ? row.providerRequestTotalSum / row.providerRequestTotalCount : null,
      avgTokens: row.tokenized ? row.tokenSum / row.tokenized : null,
      avgReasoningTokens: row.reasoningTokenized ? row.reasoningTokenSum / row.reasoningTokenized : null,
      avgOutputTokensPerSecond: row.throughputMeasured ? row.throughputSum / row.throughputMeasured : null,
      avgAttempts: row.attempted ? row.attemptSum / row.attempted : null,
      httpStatuses: formatHttpStatusCounts(row.httpStatusCounts),
      providerModels: formatTextCounts(row.providerModelCounts),
      providerModelSources: formatTextCounts(row.providerModelSourceCounts),
      providerModelCount: row.providerModelCounts.size,
      missingProviderModelRuns: Math.max(0, row.runs - sumMapValues(row.providerModelCounts)),
      configuredProviderModelRuns: row.providerModelSourceCounts.get('target_config') ?? 0,
      generationSettingCounts: row.generationSettingCounts,
      generationSettings: formatTextCounts(row.generationSettingCounts),
      generationSettingCount: row.generationSettingCounts.size,
      finishReasons: formatTextCounts(row.finishReasonCounts),
      pricingAssumptions: formatTextCounts(row.pricingAssumptionCounts),
      pricingAssumptionCount: sumMapValues(row.pricingAssumptionCounts),
      avgCostUsd: row.costed ? row.costSum / row.costed : null,
      firstRunId: row.firstRunId,
      latestStarted: row.latestStarted,
    }))
    .sort((a, b) => b.latestStarted.localeCompare(a.latestStarted) || b.passRate - a.passRate || a.targetId.localeCompare(b.targetId));
}

function buildModelIdentityWarnings(comparisonRows: ComparisonRow[]): ModelIdentityWarning[] {
  const warnings: ModelIdentityWarning[] = [];
  for (const row of comparisonRows) {
    if (row.missingProviderModelRuns > 0) {
      warnings.push({
        key: `${row.key}|provider_model_missing`,
        issue: 'provider_model_missing',
        groupId: row.groupId,
        packId: row.packId,
        targetId: row.targetId,
        runs: row.runs,
        missingProviderModelRuns: row.missingProviderModelRuns,
        configuredProviderModelRuns: row.configuredProviderModelRuns,
        providerModels: row.providerModels,
        providerModelSources: row.providerModelSources,
        note: 'Some runs did not report the served model id.',
        firstRunId: row.firstRunId,
      });
    }
    if (row.providerModelCount > 1) {
      warnings.push({
        key: `${row.key}|provider_model_inconsistent`,
        issue: 'provider_model_inconsistent',
        groupId: row.groupId,
        packId: row.packId,
        targetId: row.targetId,
        runs: row.runs,
        missingProviderModelRuns: row.missingProviderModelRuns,
        configuredProviderModelRuns: row.configuredProviderModelRuns,
        providerModels: row.providerModels,
        providerModelSources: row.providerModelSources,
        note: 'This aggregate contains multiple served model ids.',
        firstRunId: row.firstRunId,
      });
    }
    if (row.configuredProviderModelRuns > 0) {
      warnings.push({
        key: `${row.key}|provider_model_configured_fallback`,
        issue: 'provider_model_configured_fallback',
        groupId: row.groupId,
        packId: row.packId,
        targetId: row.targetId,
        runs: row.runs,
        missingProviderModelRuns: row.missingProviderModelRuns,
        configuredProviderModelRuns: row.configuredProviderModelRuns,
        providerModels: row.providerModels,
        providerModelSources: row.providerModelSources,
        note: 'Some runs used the configured target model because the provider did not echo a served model id.',
        firstRunId: row.firstRunId,
      });
    }
  }
  return warnings.sort((a, b) => a.groupId.localeCompare(b.groupId)
    || a.packId.localeCompare(b.packId)
    || a.targetId.localeCompare(b.targetId)
    || a.issue.localeCompare(b.issue));
}

function buildGenerationSettingWarnings(comparisonRows: ComparisonRow[]): GenerationSettingWarning[] {
  const warnings: GenerationSettingWarning[] = [];
  const scopeCounts = new Map<string, number>();
  for (const row of comparisonRows) {
    for (const [setting, count] of row.generationSettingCounts.entries()) {
      scopeCounts.set(setting, (scopeCounts.get(setting) ?? 0) + count);
    }
    if (row.generationSettingCount > 1) {
      warnings.push({
        key: `${row.key}|generation_settings_mixed_target`,
        issue: 'generation_settings_mixed_target',
        groupId: row.groupId,
        packId: row.packId,
        targetId: row.targetId,
        runs: row.runs,
        generationSettings: row.generationSettings,
        note: 'This target/pack aggregate mixes sampling settings; split deterministic and exploratory runs before comparing it as one model result.',
        firstRunId: row.firstRunId,
      });
    }
  }
  if (scopeCounts.size > 1) {
    warnings.push({
      key: 'all|all|all|generation_settings_mixed_scope',
      issue: 'generation_settings_mixed_scope',
      groupId: 'all',
      packId: 'all',
      targetId: 'all',
      runs: sumMapValues(scopeCounts),
      generationSettings: formatTextCounts(scopeCounts),
      note: 'The visible comparison mixes generation sampling settings; rerun or filter so one leaderboard uses the same temperature, top_p, and seed policy.',
      firstRunId: comparisonRows[0]?.firstRunId ?? '',
    });
  }
  return warnings.sort((a, b) => a.groupId.localeCompare(b.groupId)
    || a.packId.localeCompare(b.packId)
    || a.targetId.localeCompare(b.targetId)
    || a.issue.localeCompare(b.issue));
}

function buildPackEvidenceIssues(results: RunResult[], packs: BenchmarkPack[]): PackEvidenceIssue[] {
  const packIds = unionStrings(results.map(resultPackId).filter(id => id && id !== '-'));
  if (!packIds.length) {
    return [];
  }
  const packById = new Map(packs.map(pack => [pack.id, pack]));
  const issues = new Map<string, PackEvidenceIssue>();
  const snapshotPackIds = new Set<string>();

  for (const result of results) {
    const packId = resultPackId(result);
    if (!packId || packId === '-') {
      continue;
    }
    const metadata = resultPackCalibrationMetadata(result);
    const evidenceProfile = metadata?.evidenceProfile;
    if (!evidenceProfile) {
      continue;
    }
    snapshotPackIds.add(packId);
    if (promptEvidenceProfileIsPromptLike(evidenceProfile) && !promptEvidenceProfileIsComparisonReady(evidenceProfile)) {
      const pack = packById.get(packId);
      issues.set(packId, {
        packId,
        evidenceProfile,
        warnings: pack?.evidenceWarnings ?? [],
      });
    }
  }

  for (const packId of packIds) {
    if (snapshotPackIds.has(packId)) {
      continue;
    }
    const pack = packById.get(packId);
    if (!pack || !pack.taskTypes.includes('prompt')) {
      continue;
    }
    if (!promptEvidenceProfileIsComparisonReady(pack.evidenceProfile)) {
      issues.set(packId, {
        packId,
        evidenceProfile: pack.evidenceProfile,
        warnings: pack.evidenceWarnings,
      });
    }
  }

  return Array.from(issues.values()).sort((a, b) => a.packId.localeCompare(b.packId));
}

function promptEvidenceProfileIsPromptLike(profile: string) {
  return ['connectivity_smoke', 'prompt_smoke', 'weak_prompt_suite', 'thin_prompt_suite', 'prompt_comparison'].includes(profile);
}

function promptEvidenceProfileIsComparisonReady(profile: string) {
  return profile === 'prompt_comparison';
}

function buildPackCalibrationIssues(results: RunResult[]): PackCalibrationIssue[] {
  const byPack = new Map<string, {
    packId: string;
    evidenceProfiles: Set<string>;
    statuses: Set<string>;
    sampleSizes: Set<number>;
    baselineModels: Set<string>;
    lastReviewed: Set<string>;
    qualityGates: Set<string>;
    notes: Set<string>;
  }>();
  for (const result of results) {
    const packId = resultPackId(result);
    if (!packId || packId === '-') {
      continue;
    }
    const metadata = resultPackCalibrationMetadata(result);
    const row = byPack.get(packId) ?? {
      packId,
      evidenceProfiles: new Set<string>(),
      statuses: new Set<string>(),
      sampleSizes: new Set<number>(),
      baselineModels: new Set<string>(),
      lastReviewed: new Set<string>(),
      qualityGates: new Set<string>(),
      notes: new Set<string>(),
    };
    if (!metadata) {
      row.statuses.add('missing');
      row.notes.add('No calibration metadata was stored with these result rows.');
    } else {
      if (metadata.evidenceProfile) {
        row.evidenceProfiles.add(metadata.evidenceProfile);
      }
      row.statuses.add(metadata.status);
      if (metadata.sampleSize != null) {
        row.sampleSizes.add(metadata.sampleSize);
      }
      for (const model of metadata.baselineModels) {
        row.baselineModels.add(model);
      }
      if (metadata.lastReviewed) {
        row.lastReviewed.add(metadata.lastReviewed);
      }
      for (const gate of metadata.qualityGates) {
        row.qualityGates.add(gate);
      }
      if (metadata.notes) {
        row.notes.add(metadata.notes);
      }
    }
    byPack.set(packId, row);
  }

  return Array.from(byPack.values())
    .map(row => {
      const qualityGates = Array.from(row.qualityGates).sort();
      const missingQualityGates = Array.from(row.evidenceProfiles).some(profile => profile === 'prompt_comparison')
        ? missingRequiredCalibrationQualityGates(qualityGates)
        : [];
      return {
        packId: row.packId,
        statuses: Array.from(row.statuses).sort(),
        sampleSizes: Array.from(row.sampleSizes).sort((a, b) => a - b),
        baselineModels: Array.from(row.baselineModels).sort(),
        lastReviewed: Array.from(row.lastReviewed).sort(),
        qualityGates,
        missingQualityGates,
        notes: Array.from(row.notes).sort(),
      };
    })
    .filter(issue => issue.statuses.length === 0 || issue.statuses.some(status => !packCalibrationStatusIsDefinitive(status)) || issue.missingQualityGates.length > 0)
    .sort((a, b) => a.packId.localeCompare(b.packId));
}

function resultPackCalibrationMetadata(result: RunResult): {
  evidenceProfile?: string;
  status: string;
  sampleSize?: number;
  baselineModels: string[];
  lastReviewed?: string;
  qualityGates: string[];
  notes?: string;
} | null {
  const reproducibility = result.reproducibility;
  if (!isRecord(reproducibility)) {
    return null;
  }
  const pack = reproducibility.benchmark_pack ?? reproducibility.benchmarkPack;
  if (!isRecord(pack)) {
    return null;
  }
  const calibration = pack.calibration;
  if (!isRecord(calibration)) {
    return null;
  }
  const evidenceProfile = stringFromUnknown(pack.evidence_profile ?? pack.evidenceProfile);
  const status = normalizedPackCalibrationStatus(stringFromUnknown(calibration.status) ?? 'custom');
  const sampleSizeValue = calibration.sample_size ?? calibration.sampleSize;
  const sampleSize = typeof sampleSizeValue === 'number' && Number.isFinite(sampleSizeValue) && sampleSizeValue >= 0
    ? Math.round(sampleSizeValue)
    : undefined;
  const baselineModelsValue = calibration.baseline_models ?? calibration.baselineModels;
  const baselineModels = Array.isArray(baselineModelsValue)
    ? baselineModelsValue.flatMap(value => {
      const model = stringFromUnknown(value);
      return model ? [model] : [];
    })
    : [];
  const qualityGatesValue = calibration.quality_gates ?? calibration.qualityGates;
  const qualityGates = Array.isArray(qualityGatesValue)
    ? qualityGatesValue.flatMap(value => {
      const gate = stringFromUnknown(value);
      return gate ? [gate] : [];
    })
    : [];
  return {
    evidenceProfile,
    status,
    sampleSize,
    baselineModels,
    lastReviewed: stringFromUnknown(calibration.last_reviewed ?? calibration.lastReviewed),
    qualityGates,
    notes: stringFromUnknown(calibration.notes),
  };
}

function stringFromUnknown(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

function normalizedPackCalibrationStatus(status: string) {
  const normalized = status.trim().toLowerCase();
  return ['uncalibrated', 'pilot', 'reviewed', 'calibrated', 'missing'].includes(normalized)
    ? normalized
    : 'custom';
}

function packCalibrationStatusIsDefinitive(status: string) {
  return normalizedPackCalibrationStatus(status) === 'calibrated';
}

const requiredCalibrationQualityGates = [
  'local_cloud_baseline_pair',
  'provider_confirmed_model_identity',
  'complete_pack_task_coverage',
  'min_3_repetitions_per_task_target',
  'cost_metrics_for_cloud_targets',
  'single_generation_policy',
];

function missingRequiredCalibrationQualityGates(qualityGates: string[]) {
  const present = new Set(qualityGates.map(gate => gate.trim()).filter(Boolean));
  return requiredCalibrationQualityGates.filter(gate => !present.has(gate));
}

function packCalibrationNote(issues: PackCalibrationIssue[]) {
  if (!issues.length) {
    return 'All visible packs are marked calibrated and include required model-selection quality gates in the stored run metadata.';
  }
  return `Pack calibration warning: ${issues.length} pack(s) are not fully calibrated for model selection. BenchForge keeps these rankings directional and will not select a winner as comparison-ready until calibration is documented: ${packCalibrationIssueSummary(issues)}.`;
}

function packEvidenceNote(issues: PackEvidenceIssue[]) {
  if (!issues.length) {
    return 'All visible prompt packs have comparison-ready evidence profiles.';
  }
  return `Pack evidence warning: ${packEvidenceIssueSummary(issues)}. Run a prompt-comparison pack before using the ranking for model selection.`;
}

function packEvidenceIssueSummary(issues: PackEvidenceIssue[]) {
  return issues.slice(0, 4).map(issue => {
    const warning = issue.warnings[0] ? `: ${issue.warnings[0]}` : '';
    return `${issue.packId} is ${issue.evidenceProfile}${warning}`;
  }).join('; ');
}

function packCalibrationIssueSummary(issues: PackCalibrationIssue[]) {
  return issues.slice(0, 4).map(issue => {
    const detail = [
      `${issue.packId} is ${previewList(issue.statuses)}`,
      issue.sampleSizes.length ? `sample size ${formatNumberList(issue.sampleSizes)}` : '',
      issue.lastReviewed.length ? `reviewed ${previewList(issue.lastReviewed, 1)}` : '',
      issue.missingQualityGates.length ? `missing gates ${previewList(issue.missingQualityGates, 2)}` : '',
    ].filter(Boolean).join(', ');
    return detail;
  }).join('; ');
}

function formatNumberList(values: number[]) {
  return values.length ? values.map(value => value.toLocaleString()).join(', ') : '-';
}

function buildRunGroupTrendRows(comparisonRows: ComparisonRow[]): RunGroupTrendRow[] {
  const byTargetPack = new Map<string, ComparisonRow[]>();
  for (const row of comparisonRows) {
    const key = `${row.packId}|${row.targetId}`;
    const rows = byTargetPack.get(key) ?? [];
    rows.push(row);
    byTargetPack.set(key, rows);
  }
  const trends: RunGroupTrendRow[] = [];
  for (const rows of byTargetPack.values()) {
    rows.sort((a, b) => b.latestStarted.localeCompare(a.latestStarted) || b.groupId.localeCompare(a.groupId));
    const current = rows[0];
    const previous = rows[1];
    if (!current || !previous) {
      continue;
    }
    const passRateDelta = current.passRate - previous.passRate;
    const avgScoreDelta = optionalDelta(current.avgScore, previous.avgScore);
    const p95TimeDeltaMs = optionalDelta(current.p95TimeMs, previous.p95TimeMs);
    const avgCostDeltaUsd = optionalDelta(current.avgCostUsd, previous.avgCostUsd);
    const { level, signal } = runGroupTrendSignal(passRateDelta, avgScoreDelta, current.p95TimeMs, previous.p95TimeMs, current.avgCostUsd, previous.avgCostUsd);
    trends.push({
      key: `${current.packId}|${current.targetId}|${current.groupId}|${previous.groupId}`,
      packId: current.packId,
      targetId: current.targetId,
      current,
      previous,
      passRateDelta,
      avgScoreDelta,
      p95TimeDeltaMs,
      avgCostDeltaUsd,
      signalLevel: level,
      signal,
    });
  }
  return trends.sort((a, b) => trendLevelRank(a.signalLevel) - trendLevelRank(b.signalLevel)
    || b.current.latestStarted.localeCompare(a.current.latestStarted)
    || a.packId.localeCompare(b.packId)
    || a.targetId.localeCompare(b.targetId));
}

function buildTargetRankingRows(results: RunResult[], adapterById: Map<string, Adapter>): TargetRankingRow[] {
  const groups = new Map<string, {
    targetId: string;
    providerCounts: Map<string, number>;
    groupIds: Set<string>;
    packIds: Set<string>;
    taskIds: Set<string>;
    packTaskSlots: Set<string>;
    runs: number;
    passed: number;
    totalTaskWeight: number;
    weightedPassed: number;
    scoredWeight: number;
    weightedScoreSum: number;
    scoreValues: number[];
    timeValues: number[];
    costValues: number[];
    pricingAssumptionRuns: number;
    pricingAssumptionIds: Set<string>;
    throughputValues: number[];
    errorCodeCounts: Map<string, number>;
    firstRunId: string;
    latestStarted: string;
  }>();
  for (const result of results) {
    const targetId = resultTargetId(result);
    const row = groups.get(targetId) ?? {
      targetId,
      providerCounts: new Map<string, number>(),
      groupIds: new Set<string>(),
      packIds: new Set<string>(),
      taskIds: new Set<string>(),
      packTaskSlots: new Set<string>(),
      runs: 0,
      passed: 0,
      totalTaskWeight: 0,
      weightedPassed: 0,
      scoredWeight: 0,
      weightedScoreSum: 0,
      scoreValues: [] as number[],
      timeValues: [] as number[],
      costValues: [] as number[],
      pricingAssumptionRuns: 0,
      pricingAssumptionIds: new Set<string>(),
      throughputValues: [] as number[],
      errorCodeCounts: new Map<string, number>(),
      firstRunId: result.id,
      latestStarted: result.started_at ?? '',
    };
    row.runs += 1;
    const taskWeight = resultTaskWeight(result);
    row.totalTaskWeight += taskWeight;
    row.groupIds.add(resultGroupId(result));
    const packId = resultPackId(result);
    const taskId = resultTaskId(result);
    row.packIds.add(packId);
    row.taskIds.add(taskId);
    row.packTaskSlots.add(packTaskSlotId(packId, taskId));
    const provider = resultProviderLabel(result, adapterById);
    row.providerCounts.set(provider, (row.providerCounts.get(provider) ?? 0) + 1);
    if (result.status === 'passed') {
      row.passed += 1;
      row.weightedPassed += taskWeight;
    } else {
      const code = result.error_code || result.status || 'unknown';
      row.errorCodeCounts.set(code, (row.errorCodeCounts.get(code) ?? 0) + 1);
    }
    if (typeof result.score === 'number') {
      row.scoreValues.push(result.score);
      row.scoredWeight += taskWeight;
      row.weightedScoreSum += result.score * taskWeight;
    }
    const wallTime = resultWallTimeMs(result);
    if (typeof wallTime === 'number') {
      row.timeValues.push(wallTime);
    }
    const costUsd = resultCostUsdForCoverage(result);
    if (costUsd != null) {
      row.costValues.push(costUsd);
    }
    const pricingAssumption = result.pricing_assumption?.trim();
    if (pricingAssumption) {
      row.pricingAssumptionRuns += 1;
      row.pricingAssumptionIds.add(pricingAssumption);
    }
    if (typeof result.output_tokens_per_second === 'number') {
      row.throughputValues.push(result.output_tokens_per_second);
    }
    if ((result.started_at ?? '') > row.latestStarted) {
      row.latestStarted = result.started_at ?? '';
      row.firstRunId = result.id;
    }
    groups.set(targetId, row);
  }
  return Array.from(groups.values())
    .map(row => ({
      targetId: row.targetId,
      providers: formatTextCounts(row.providerCounts),
      runs: row.runs,
      passed: row.passed,
      passRate: row.runs ? row.passed / row.runs : 0,
      totalTaskWeight: row.totalTaskWeight,
      weightedPassRate: row.totalTaskWeight ? row.weightedPassed / row.totalTaskWeight : null,
      weightedAvgScore: row.scoredWeight ? row.weightedScoreSum / row.scoredWeight : null,
      passRateCiLow: passRateInterval(row.passed, row.runs)?.low ?? null,
      passRateCiHigh: passRateInterval(row.passed, row.runs)?.high ?? null,
      avgScore: average(row.scoreValues),
      scoreStdDev: stdDev(row.scoreValues),
      medianScore: median(row.scoreValues),
      minScore: minValue(row.scoreValues),
      maxScore: maxValue(row.scoreValues),
      p95TimeMs: percentile(row.timeValues, 0.95),
      medianTimeMs: median(row.timeValues),
      minTimeMs: minValue(row.timeValues),
      maxTimeMs: maxValue(row.timeValues),
      avgCostUsd: average(row.costValues),
      costedRuns: row.costValues.length,
      pricingAssumptionRuns: row.pricingAssumptionRuns,
      pricingAssumptionIds: Array.from(row.pricingAssumptionIds).sort(),
      avgOutputTokensPerSecond: average(row.throughputValues),
      groups: row.groupIds.size,
      packs: row.packIds.size,
      tasks: row.taskIds.size,
      groupIds: Array.from(row.groupIds).sort(),
      packIds: Array.from(row.packIds).sort(),
      taskIds: Array.from(row.taskIds).sort(),
      packTaskSlots: Array.from(row.packTaskSlots).sort(),
      errorCodes: formatTextCountsWithSingles(row.errorCodeCounts),
      firstRunId: row.firstRunId,
      latestStarted: row.latestStarted,
    }))
    .sort(compareTargetRankingRows);
}

function buildTaskComparisonRows(results: RunResult[]): TaskComparisonRow[] {
  const groups = new Map<string, {
    key: string;
    groupId: string;
    packId: string;
    taskId: string;
    targetId: string;
    runs: number;
    passed: number;
    scoreSum: number;
    scored: number;
    timeSum: number;
    timed: number;
    providerTimeToFirstTokenSum: number;
    providerTimeToFirstTokenCount: number;
    tokenSum: number;
    tokenized: number;
    throughputSum: number;
    throughputMeasured: number;
    httpStatusCounts: Map<number, number>;
    errorCodeCounts: Map<string, number>;
    costSum: number;
    costed: number;
    scoreValues: number[];
    timeValues: number[];
    firstRunId: string;
    latestStarted: string;
  }>();
  for (const result of results) {
    const groupId = resultGroupId(result);
    const packId = resultPackId(result);
    const taskId = resultTaskId(result);
    const targetId = resultTargetId(result);
    const key = `${groupId}|${packId}|${taskId}|${targetId}`;
    const row = groups.get(key) ?? {
      key,
      groupId,
      packId,
      taskId,
      targetId,
      runs: 0,
      passed: 0,
      scoreSum: 0,
      scored: 0,
      timeSum: 0,
      timed: 0,
      providerTimeToFirstTokenSum: 0,
      providerTimeToFirstTokenCount: 0,
      tokenSum: 0,
      tokenized: 0,
      throughputSum: 0,
      throughputMeasured: 0,
      httpStatusCounts: new Map<number, number>(),
      errorCodeCounts: new Map<string, number>(),
      costSum: 0,
      costed: 0,
      scoreValues: [] as number[],
      timeValues: [] as number[],
      firstRunId: result.id,
      latestStarted: result.started_at ?? '',
    };
    row.runs += 1;
    if (result.status === 'passed') {
      row.passed += 1;
    }
    if (typeof result.score === 'number') {
      row.scoreSum += result.score;
      row.scored += 1;
      row.scoreValues.push(result.score);
    }
    const wallTime = resultWallTimeMs(result);
    if (typeof wallTime === 'number') {
      row.timeSum += wallTime;
      row.timed += 1;
      row.timeValues.push(wallTime);
    }
    if (typeof result.provider_time_to_first_token_ms === 'number') {
      row.providerTimeToFirstTokenSum += result.provider_time_to_first_token_ms;
      row.providerTimeToFirstTokenCount += 1;
    }
    const tokens = totalTokens(result);
    if (tokens != null) {
      row.tokenSum += tokens;
      row.tokenized += 1;
    }
    if (typeof result.output_tokens_per_second === 'number') {
      row.throughputSum += result.output_tokens_per_second;
      row.throughputMeasured += 1;
    }
    if (typeof result.http_status === 'number' && Number.isFinite(result.http_status)) {
      const status = Math.round(result.http_status);
      if (status >= 100 && status <= 599) {
        row.httpStatusCounts.set(status, (row.httpStatusCounts.get(status) ?? 0) + 1);
      }
    }
    if (result.status !== 'passed') {
      const code = result.error_code || result.status || 'unknown';
      row.errorCodeCounts.set(code, (row.errorCodeCounts.get(code) ?? 0) + 1);
    }
    const costUsd = resultCostUsdForCoverage(result);
    if (costUsd != null) {
      row.costSum += costUsd;
      row.costed += 1;
    }
    if ((result.started_at ?? '') > row.latestStarted) {
      row.latestStarted = result.started_at ?? '';
      row.firstRunId = result.id;
    }
    groups.set(key, row);
  }
  return Array.from(groups.values())
    .map(row => ({
      key: row.key,
      groupId: row.groupId,
      packId: row.packId,
      taskId: row.taskId,
      targetId: row.targetId,
      runs: row.runs,
      passed: row.passed,
      passRate: row.runs ? row.passed / row.runs : 0,
      avgScore: row.scored ? row.scoreSum / row.scored : null,
      scoreStdDev: stdDev(row.scoreValues),
      medianScore: median(row.scoreValues),
      minScore: minValue(row.scoreValues),
      maxScore: maxValue(row.scoreValues),
      avgTimeMs: row.timed ? row.timeSum / row.timed : null,
      p95TimeMs: percentile(row.timeValues, 0.95),
      medianTimeMs: median(row.timeValues),
      minTimeMs: minValue(row.timeValues),
      maxTimeMs: maxValue(row.timeValues),
      avgProviderTimeToFirstTokenMs: row.providerTimeToFirstTokenCount ? row.providerTimeToFirstTokenSum / row.providerTimeToFirstTokenCount : null,
      avgTokens: row.tokenized ? row.tokenSum / row.tokenized : null,
      avgOutputTokensPerSecond: row.throughputMeasured ? row.throughputSum / row.throughputMeasured : null,
      httpStatuses: formatHttpStatusCounts(row.httpStatusCounts),
      errorCodes: formatTextCounts(row.errorCodeCounts),
      avgCostUsd: row.costed ? row.costSum / row.costed : null,
      firstRunId: row.firstRunId,
      latestStarted: row.latestStarted,
    }))
    .sort((a, b) => a.passRate - b.passRate
      || (a.avgScore ?? Number.POSITIVE_INFINITY) - (b.avgScore ?? Number.POSITIVE_INFINITY)
      || (b.p95TimeMs ?? -1) - (a.p95TimeMs ?? -1)
      || a.taskId.localeCompare(b.taskId)
      || a.targetId.localeCompare(b.targetId));
}

function buildTaskTargetMatrix(results: RunResult[]): TaskTargetMatrix {
  const targets = uniqueSorted(results.map(resultTargetId));
  const rows = new Map<string, {
    key: string;
    groupId: string;
    packId: string;
    taskId: string;
    cells: Record<string, {
      runs: number;
      passed: number;
      scoreValues: number[];
      timeValues: number[];
      errorCodeCounts: Map<string, number>;
      firstRunId: string;
      latestStarted: string;
    }>;
  }>();
  for (const result of results) {
    const groupId = resultGroupId(result);
    const packId = resultPackId(result);
    const taskId = resultTaskId(result);
    const targetId = resultTargetId(result);
    const key = `${groupId}|${packId}|${taskId}`;
    const row = rows.get(key) ?? { key, groupId, packId, taskId, cells: {} };
    const cell = row.cells[targetId] ?? {
      runs: 0,
      passed: 0,
      scoreValues: [] as number[],
      timeValues: [] as number[],
      errorCodeCounts: new Map<string, number>(),
      firstRunId: result.id,
      latestStarted: result.started_at ?? '',
    };
    cell.runs += 1;
    if (result.status === 'passed') {
      cell.passed += 1;
    } else {
      const code = result.error_code || result.status || 'unknown';
      cell.errorCodeCounts.set(code, (cell.errorCodeCounts.get(code) ?? 0) + 1);
    }
    if (typeof result.score === 'number') {
      cell.scoreValues.push(result.score);
    }
    const wallTime = resultWallTimeMs(result);
    if (typeof wallTime === 'number') {
      cell.timeValues.push(wallTime);
    }
    if ((result.started_at ?? '') > cell.latestStarted) {
      cell.latestStarted = result.started_at ?? '';
      cell.firstRunId = result.id;
    }
    row.cells[targetId] = cell;
    rows.set(key, row);
  }
  return {
    targets,
    rows: Array.from(rows.values())
      .map(row => ({
        key: row.key,
        groupId: row.groupId,
        packId: row.packId,
        taskId: row.taskId,
        cells: Object.fromEntries(Object.entries(row.cells).map(([target, cell]) => [target, {
          runs: cell.runs,
          passed: cell.passed,
          passRate: cell.runs ? cell.passed / cell.runs : 0,
          avgScore: average(cell.scoreValues),
          p95TimeMs: percentile(cell.timeValues, 0.95),
          errorCodes: formatTextCountsWithSingles(cell.errorCodeCounts),
          firstRunId: cell.firstRunId,
        } satisfies TaskTargetMatrixCell])),
      }))
      .sort((a, b) => a.groupId.localeCompare(b.groupId) || a.packId.localeCompare(b.packId) || a.taskId.localeCompare(b.taskId)),
  };
}

function buildDecisionSnapshot(comparisonRows: ComparisonRow[], taskRows: TaskComparisonRow[], targetRankingRows: TargetRankingRow[], packEvidenceIssues: PackEvidenceIssue[], packCalibrationIssues: PackCalibrationIssue[]): DecisionSnapshot | null {
  if (!comparisonRows.length || !targetRankingRows.length) {
    return null;
  }
  const sorted = [...comparisonRows].sort(compareDecisionRows);
  const bestOverall = sorted[0];
  const recommendedTarget = targetRankingRows[0];
  const closeContenders = closeTargetContenders(targetRankingRows);
  const bestPassRate = bestOverall.passRate;
  const reliableRows = comparisonRows.filter(row => row.passRate === bestPassRate);
  const fastestReliable = minBy(
    reliableRows.filter(row => row.p95TimeMs != null || row.avgTimeMs != null),
    row => row.p95TimeMs ?? row.avgTimeMs ?? Number.POSITIVE_INFINITY,
  );
  const cheapestReliable = minBy(
    reliableRows.filter(row => row.avgCostUsd != null),
    row => row.avgCostUsd ?? Number.POSITIVE_INFINITY,
  );
  const throughputLeader = maxBy(
    reliableRows.filter(row => row.avgOutputTokensPerSecond != null),
    row => row.avgOutputTokensPerSecond ?? Number.NEGATIVE_INFINITY,
  );
  const weakestTask = taskRows[0];
  const coverageNote = targetCoverageParityNote(targetRankingRows);
  const scoreStabilityNote = targetScoreStabilityNote(targetRankingRows);
  const confidenceNote = targetConfidenceNote(targetRankingRows, comparisonRows, taskRows);
  const evidence = comparisonEvidenceAssessment(comparisonRows, taskRows, targetRankingRows, packEvidenceIssues, packCalibrationIssues);
  const decisionStatus = evidenceDecisionStatus(evidence);
  const selectedTargetId = evidenceSelectedTargetId(evidence, targetRankingRows);
  return {
    decisionStatus,
    selectedTargetId,
    selectionNote: evidenceSelectionNote(evidence, targetRankingRows),
    recommendedTarget,
    closeContenders,
    bestOverall,
    fastestReliable,
    cheapestReliable,
    throughputLeader,
    weakestTask,
    confidenceNote,
    coverageNote,
    evidenceGrade: evidence.grade,
    evidenceLabel: evidence.label,
    evidenceNote: evidence.note,
    packEvidenceIssues,
    packCalibrationIssues,
    calibrationNote: packCalibrationNote(packCalibrationIssues),
    minimumNextRun: evidence.minimumNextRun,
    scoreStabilityNote,
  };
}

const recommendedTaskRepetitions = 3;

function closeTargetContenders(rows: TargetRankingRow[]) {
  const leader = rows[0];
  if (!leader) {
    return [];
  }
  return rows.slice(1).filter(row => targetQualityTie(leader, row));
}

function targetQualityTie(a: TargetRankingRow, b: TargetRankingRow) {
  return Math.abs(a.passRate - b.passRate) < 0.0000001
    && nullableNumbersEqual(a.avgScore, b.avgScore);
}

function nullableNumbersEqual(a: number | null, b: number | null) {
  if (a == null || b == null) {
    return a == null && b == null;
  }
  return Math.abs(a - b) < 0.0000001;
}

function optionalDelta(current: number | null, previous: number | null) {
  return current == null || previous == null ? null : current - previous;
}

function runGroupTrendSignal(passRateDelta: number, avgScoreDelta: number | null, currentP95Ms: number | null, previousP95Ms: number | null, currentCostUsd: number | null, previousCostUsd: number | null): { level: 'ok' | 'warn'; signal: string } {
  const regressions: string[] = [];
  const improvements: string[] = [];
  if (passRateDelta <= -0.05) {
    regressions.push(`pass rate ${formatPercentPointDelta(passRateDelta)}`);
  } else if (passRateDelta >= 0.05) {
    improvements.push(`pass rate ${formatPercentPointDelta(passRateDelta)}`);
  }
  if (avgScoreDelta != null) {
    if (avgScoreDelta <= -0.05) {
      regressions.push(`score ${formatNumberDelta(avgScoreDelta)}`);
    } else if (avgScoreDelta >= 0.05) {
      improvements.push(`score ${formatNumberDelta(avgScoreDelta)}`);
    }
  }
  if (currentP95Ms != null && previousP95Ms != null && previousP95Ms > 0) {
    const delta = currentP95Ms - previousP95Ms;
    if (delta > 0 && delta / previousP95Ms >= 0.2) {
      regressions.push(`p95 latency ${formatMsDelta(delta)}`);
    } else if (delta < 0 && Math.abs(delta) / previousP95Ms >= 0.2) {
      improvements.push(`p95 latency ${formatMsDelta(delta)}`);
    }
  }
  if (currentCostUsd != null && previousCostUsd != null && previousCostUsd > 0) {
    const delta = currentCostUsd - previousCostUsd;
    if (delta > 0 && delta / previousCostUsd >= 0.2) {
      regressions.push(`avg cost ${formatCostDelta(delta)}`);
    } else if (delta < 0 && Math.abs(delta) / previousCostUsd >= 0.2) {
      improvements.push(`avg cost ${formatCostDelta(delta)}`);
    }
  }
  if (regressions.length) {
    return { level: 'warn', signal: `regression: ${regressions.join('; ')}` };
  }
  if (improvements.length) {
    return { level: 'ok', signal: `improvement: ${improvements.join('; ')}` };
  }
  return { level: 'ok', signal: 'stable' };
}

function trendLevelRank(level: 'ok' | 'warn') {
  return level === 'warn' ? 0 : 1;
}

function compareTargetRankingRows(a: TargetRankingRow, b: TargetRankingRow) {
  return nullNumberDesc(a.weightedPassRate, b.weightedPassRate)
    || b.passRate - a.passRate
    || nullNumberDesc(a.weightedAvgScore, b.weightedAvgScore)
    || nullNumberDesc(a.avgScore, b.avgScore)
    || nullNumberAsc(a.scoreStdDev, b.scoreStdDev)
    || nullNumberAsc(a.p95TimeMs, b.p95TimeMs)
    || nullNumberAsc(a.avgCostUsd, b.avgCostUsd)
    || nullNumberDesc(a.avgOutputTokensPerSecond, b.avgOutputTokensPerSecond)
    || b.runs - a.runs
    || a.targetId.localeCompare(b.targetId);
}

function targetScoreStabilityNote(rows: TargetRankingRow[]) {
  const measured = rows.filter(row => row.scoreStdDev != null);
  if (!measured.length) {
    return 'Run at least 2 scored repetitions per target to measure score stability.';
  }
  const highest = measured.reduce((worst, row) => (row.scoreStdDev ?? 0) > (worst.scoreStdDev ?? 0) ? row : worst, measured[0]);
  if ((highest.scoreStdDev ?? 0) < 0.0000001) {
    return `All ${measured.length} target(s) with at least 2 scored runs have score σ 0 in the current scope.`;
  }
  return `Max target score spread is σ ${formatNumber(highest.scoreStdDev)} on ${highest.targetId}; lower spread means more consistent scores across the current scope.`;
}

function targetConfidenceNote(targetRows: TargetRankingRow[], comparisonRows: ComparisonRow[], taskRows: TaskComparisonRow[]) {
  const notes: string[] = [];
  const overlapTargets = passRateCiOverlapTargetIds(targetRows);
  if (overlapTargets.length) {
    notes.push(`Pass-rate confidence warning: the recommended target's Wilson 95% interval overlaps ${overlapTargets.length} target(s): ${overlapTargets.join(', ')}; treat the ranking as provisional and run more repetitions.`);
  } else if (targetRows.length > 1) {
    notes.push('The recommended target\'s Wilson 95% pass-rate interval is separated from the other visible targets.');
  }
  if (taskRows.length) {
    notes.push(taskRepetitionNote(taskRows));
  }
  const lowSampleRows = comparisonRows.filter(row => row.runs < 3).length;
  if (lowSampleRows) {
    notes.push(`${lowSampleRows}/${comparisonRows.length} comparison row(s) have fewer than 3 measured runs; use repetitions for higher confidence.`);
  } else if (comparisonRows.length) {
    notes.push('All comparison rows in scope have at least 3 measured runs.');
  }
  return notes.join(' ');
}

function taskRepetitionNote(taskRows: TaskComparisonRow[]) {
  const low = taskRows.filter(row => row.runs < recommendedTaskRepetitions).length;
  if (low) {
    return `${low}/${taskRows.length} task-target row(s) have fewer than ${recommendedTaskRepetitions} measured repetitions for the same task and target; add repetitions to separate task breadth from repeatability.`;
  }
  return `All task-target rows in scope have at least ${recommendedTaskRepetitions} measured repetitions.`;
}

function passRateCiOverlapTargetIds(rows: TargetRankingRow[]) {
  const leader = rows[0];
  if (!leader || leader.passRateCiLow == null || leader.passRateCiHigh == null) {
    return [];
  }
  return rows.slice(1)
    .filter(row => row.passRateCiLow != null
      && row.passRateCiHigh != null
      && intervalsOverlap(leader.passRateCiLow!, leader.passRateCiHigh!, row.passRateCiLow!, row.passRateCiHigh!))
    .map(row => row.targetId);
}

function intervalsOverlap(leftLow: number, leftHigh: number, rightLow: number, rightHigh: number) {
  return leftLow <= rightHigh && rightLow <= leftHigh;
}

function targetCoverageParityNote(rows: TargetRankingRow[]) {
  if (rows.length < 2) {
    return 'Only one target is visible; add another target to compare coverage.';
  }
  const gaps = targetCoverageIssues(rows);
  if (gaps.length) {
    const worst = gaps[0];
    return `Coverage warning: ${gaps.length}/${rows.length} target(s) do not cover every pack/task slot in scope; largest gap is ${worst.targetId} missing ${worst.missingPackTaskSlots.length} pack/task slot(s), ${worst.missingTasks.length} task(s), and ${worst.missingPacks.length} pack(s).`;
  }
  if (!targetsCoverSameRunGroups(rows)) {
    return 'Coverage note: targets cover the same pack/task slots but not the same run groups; same-run comparisons are more controlled.';
  }
  return 'All targets cover the same pack/task slots and run groups in the current scope.';
}

function buildResultEvidenceSummary(comparisonRows: ComparisonRow[], taskRows: TaskComparisonRow[], targetRows: TargetRankingRow[], targets: Target[], packs: BenchmarkPack[], packEvidenceIssues: PackEvidenceIssue[] = [], packCalibrationIssues: PackCalibrationIssue[] = []): ResultEvidenceSummary | null {
  if (!comparisonRows.length && !targetRows.length) {
    return null;
  }
  const evidence = comparisonEvidenceAssessment(comparisonRows, taskRows, targetRows, packEvidenceIssues, packCalibrationIssues);
  const targetById = new Map(targets.map(target => [target.id, target]));
  const pricingRepairTargetIds = evidencePricingRepairTargetIds(evidence, targetRows, targetById);

  return {
    tone: evidence.tone,
    grade: evidence.grade,
    label: evidence.label,
    headline: evidence.note,
    notes: evidence.notes,
    coverageIssues: evidence.coverageIssues,
    risks: evidence.risks,
    minimumNextRun: evidence.minimumNextRun,
    nextRunIntent: pricingRepairTargetIds.length ? null : evidenceNextRunIntent(evidence, targetRows, targets, packs),
    pricingRepairTargetIds,
  };
}

function evidenceNextRunIntent(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[], targets: Target[], packs: BenchmarkPack[]): RunBuilderIntent | null {
  if (evidence.grade === 'comparison_ready') {
    return null;
  }
  const targetById = new Map(targets.map(target => [target.id, target]));
  if (evidencePricingRepairTargetIds(evidence, targetRows, targetById).length) {
    return null;
  }
  const coverageFollowUp = coverageTaskFollowUp(evidence);
  const visibleTargetIds = new Set(
    coverageFollowUp?.targetIds ?? targetRows.map(row => row.targetId).filter(id => id && id !== '-')
  );
  const runnableTargets = targets
    .filter(target => visibleTargetIds.has(target.id))
    .filter(targetIsSelectableModel);
  if (coverageFollowUp) {
    if (!runnableTargets.length) {
      return null;
    }
    return {
      ...localCloudRunBuilderIntent(unionStrings(runnableTargets.map(target => target.id)), coverageFollowUp.packId),
      taskIds: coverageFollowUp.taskIds,
    };
  }
  const localCount = runnableTargets.filter(isLocalModelTarget).length;
  const cloudCount = runnableTargets.filter(isCloudModelTarget).length;
  if (!localCount || !cloudCount) {
    return null;
  }
  const targetIds = unionStrings(runnableTargets.map(target => target.id));
  return localCloudRunBuilderIntent(targetIds, evidenceNextPackId(evidence, targetRows, packs));
}

function evidencePricingRepairTargetIds(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[], targetById: Map<string, Target>) {
  return unionStrings([
    ...costCoverageRepairTargetIds(evidence, targetRows, targetById),
    ...pricingAssumptionRepairTargetIds(evidence, targetRows, targetById),
  ]).sort();
}

function costCoverageRepairTargetIds(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[], targetById: Map<string, Target>) {
  if (!evidence.risks.includes('cost_coverage_gap')) {
    return [];
  }
  return targetRows
    .filter(row => row.runs > 0 && row.costedRuns < row.runs)
    .map(row => targetById.get(row.targetId))
    .filter((target): target is Target => Boolean(target))
    .filter(target => isCloudModelTarget(target) && !targetHasInputOutputPricing(target))
    .map(target => target.id)
    .sort();
}

function pricingAssumptionRepairTargetIds(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[], targetById: Map<string, Target>) {
  if (!evidence.risks.includes('pricing_assumption')) {
    return [];
  }
  return targetRows
    .filter(row => row.pricingAssumptionRuns > 0)
    .filter(row => !targetHasPromptCachePricingForAssumptions(targetById.get(row.targetId), row.pricingAssumptionIds))
    .map(row => row.targetId)
    .sort();
}

function targetHasPromptCachePricingForAssumptions(target: Target | undefined, assumptionIds: string[]) {
  if (!target) {
    return false;
  }
  const normalized = assumptionIds.map(id => id.toLowerCase());
  const mentionsCacheRead = normalized.some(id => id.includes('cache_read') || id.includes('cached_input'));
  const mentionsCacheWrite = normalized.some(id => id.includes('cache_write') || id.includes('cache_creation'));
  const unknownCachePricingAssumption = normalized.length > 0 && !mentionsCacheRead && !mentionsCacheWrite;
  const needsCacheRead = !normalized.length || mentionsCacheRead || unknownCachePricingAssumption;
  const needsCacheWrite = !normalized.length || mentionsCacheWrite || unknownCachePricingAssumption;
  if (needsCacheRead && !targetPriceIsConfigured(target.cacheReadPriceUsdPerMillionTokens)) {
    return false;
  }
  if (needsCacheWrite && !targetPriceIsConfigured(target.cacheWritePriceUsdPerMillionTokens)) {
    return false;
  }
  return true;
}

function targetPriceIsConfigured(value: number | null | undefined) {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function targetHasInputOutputPricing(target: Target) {
  return targetPriceIsConfigured(target.inputPriceUsdPerMillionTokens)
    && targetPriceIsConfigured(target.outputPriceUsdPerMillionTokens);
}

function targetListWithOverride(target: Target, targets: Target[]) {
  return [...targets.filter(candidate => candidate.id !== target.id), target];
}

function firstEditableTargetById(targetIds: string[], targets: Target[]) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  return targetIds
    .map(id => targetById.get(id))
    .find((target): target is Target => Boolean(target && targetRepairTargetCanEdit(target)));
}

function targetLabelsById(targetIds: string[], targets: Target[]) {
  const targetById = new Map(targets.map(target => [target.id, target]));
  return targetIds.map(id => targetById.get(id)?.name || id);
}

function cappedIntentHasUnpricedCloudTarget(intent: RunBuilderIntent, targets: Target[]) {
  if (typeof intent.maxCostUsd !== 'number' || !Number.isFinite(intent.maxCostUsd)) {
    return false;
  }
  return unpricedCloudTargetIdsForIntent(intent, targets).length > 0;
}

function unpricedCloudTargetIdsForIntent(intent: RunBuilderIntent | null, targets: Target[]) {
  if (!intent) {
    return [];
  }
  const targetById = new Map(targets.map(target => [target.id, target]));
  return intent.targetIds.filter(targetId => {
    const target = targetById.get(targetId);
    return Boolean(target && isCloudModelTarget(target) && !targetHasInputOutputPricing(target));
  });
}

function coverageTaskFollowUp(evidence: ComparisonEvidenceAssessment): { packId: string; taskIds: string[]; targetIds: string[] } | null {
  if (!evidence.coverageIssues.length) {
    return null;
  }
  const packIds = new Set<string>();
  const taskIds = new Set<string>();
  const targetIds: string[] = [];
  for (const issue of evidence.coverageIssues) {
    let hasMissingSlot = false;
    for (const slot of issue.missingPackTaskSlots) {
      const parsed = parsePackTaskSlotId(slot);
      if (!parsed) {
        return null;
      }
      packIds.add(parsed.packId);
      taskIds.add(parsed.taskId);
      hasMissingSlot = true;
    }
    if (hasMissingSlot && issue.targetId && issue.targetId !== '-') {
      targetIds.push(issue.targetId);
    }
  }
  if (packIds.size !== 1 || !taskIds.size || !targetIds.length) {
    return null;
  }
  return {
    packId: Array.from(packIds)[0],
    taskIds: Array.from(taskIds).sort(),
    targetIds: unionStrings(targetIds),
  };
}

function parsePackTaskSlotId(slot: string) {
  const separator = slot.indexOf('/');
  if (separator <= 0 || separator >= slot.length - 1) {
    return null;
  }
  return {
    packId: slot.slice(0, separator),
    taskId: slot.slice(separator + 1),
  };
}

function evidenceNextPackId(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[], packs: BenchmarkPack[]) {
  const packById = new Map(packs.map(pack => [pack.id, pack]));
  const packIds = unionStrings(targetRows.flatMap(row => row.packIds))
    .filter(id => id && id !== '-' && id !== connectivityBenchmarkPackId);
  const comparisonPackIds = packIds.filter(id => {
    const pack = packById.get(id);
    return pack ? promptEvidenceProfileIsComparisonReady(pack.evidenceProfile) : true;
  });
  if (comparisonPackIds.length && !evidence.risks.includes('pack_evidence_profile')) {
    return comparisonPackIds[0];
  }
  return preferredEvidenceFollowUpPackId(packs);
}

function preferredEvidenceFollowUpPackId(packs: BenchmarkPack[]) {
  const packById = new Map(packs.map(pack => [pack.id, pack]));
  for (const packId of ['llm-reliability', 'llm-decision-suite', 'llm-practical', 'llm-grounded-context', 'llm-structured-output', 'llm-core', defaultModelComparisonPackId]) {
    const pack = packById.get(packId);
    if (pack && pack.taskTypes.includes('prompt') && promptEvidenceProfileIsComparisonReady(pack.evidenceProfile)) {
      return pack.id;
    }
  }
  const promptComparisonPack = packs.find(pack => pack.taskTypes.includes('prompt') && promptEvidenceProfileIsComparisonReady(pack.evidenceProfile));
  if (promptComparisonPack) {
    return promptComparisonPack.id;
  }
  return 'llm-reliability';
}

function formatDecisionStatus(status: ModelSelectionDecisionStatus) {
  switch (status) {
    case 'select_recommended_target':
      return 'Select recommended target';
    case 'collect_more_evidence':
      return 'Collect more evidence';
    case 'insufficient_evidence':
      return 'Insufficient evidence';
  }
}

interface ComparisonEvidenceAssessment {
  tone: 'ok' | 'warn';
  grade: ComparisonEvidenceGrade;
  label: string;
  note: string;
  notes: string[];
  coverageIssues: ResultCoverageIssue[];
  risks: string[];
  minimumNextRun: string;
}

function comparisonEvidenceAssessment(comparisonRows: ComparisonRow[], taskRows: TaskComparisonRow[], targetRows: TargetRankingRow[], packEvidenceIssues: PackEvidenceIssue[] = [], packCalibrationIssues: PackCalibrationIssue[] = []): ComparisonEvidenceAssessment {
  const coverageIssues = targetCoverageIssues(targetRows);
  const overlapTargets = passRateCiOverlapTargetIds(targetRows);
  const costGapTargets = targetCostCoverageGapIds(targetRows);
  const pricingAssumptionTargets = targetPricingAssumptionIds(targetRows);
  const modelIdentityWarnings = buildModelIdentityWarnings(comparisonRows);
  const generationSettingWarnings = buildGenerationSettingWarnings(comparisonRows);
  const modelIdentityMissing = modelIdentityWarnings.some(warning => warning.issue === 'provider_model_missing');
  const modelIdentityInconsistent = modelIdentityWarnings.some(warning => warning.issue === 'provider_model_inconsistent');
  const modelIdentityFallback = modelIdentityWarnings.some(warning => warning.issue === 'provider_model_configured_fallback');
  const lowTaskRows = taskRows.filter(row => row.runs < recommendedTaskRepetitions).length;
  const lowComparisonRows = comparisonRows.filter(row => row.runs < recommendedTaskRepetitions).length;
  const sameRunGroups = targetsCoverSameRunGroups(targetRows);
  const connectivityOnly = comparisonScopeIsConnectivityOnly(targetRows);
  const scope = `${targetRows.length} target(s), ${unionStrings(targetRows.flatMap(row => row.packTaskSlots)).length} pack/task slot(s), ${taskRows.length} task-target row(s), ${comparisonRows.length} comparison row(s)`;
  const risks: string[] = [];
  if (!comparisonRows.length || !targetRows.length) {
    risks.push('no_comparison_results');
  }
  if (targetRows.length < 2) {
    risks.push('single_target');
  }
  if (connectivityOnly) {
    risks.push('connectivity_pack_only');
  }
  if (packEvidenceIssues.length) {
    risks.push('pack_evidence_profile');
  }
  if (lowTaskRows || lowComparisonRows) {
    risks.push('low_repetitions');
  }
  if (coverageIssues.length) {
    risks.push('coverage_gap');
  }
  if (!sameRunGroups) {
    risks.push('separate_run_groups');
  }
  if (overlapTargets.length) {
    risks.push('pass_rate_ci_overlap');
  }
  if (costGapTargets.length) {
    risks.push('cost_coverage_gap');
  }
  if (pricingAssumptionTargets.length) {
    risks.push('pricing_assumption');
  }
  if (modelIdentityMissing) {
    risks.push('provider_model_missing');
  }
  if (modelIdentityInconsistent) {
    risks.push('provider_model_inconsistent');
  }
  if (modelIdentityFallback) {
    risks.push('provider_model_configured_fallback');
  }
  if (generationSettingWarnings.length) {
    risks.push('generation_settings_mixed');
  }
  if (packCalibrationIssues.length) {
    risks.push('pack_calibration');
  }

  if (!comparisonRows.length || !targetRows.length) {
    return {
      tone: 'warn',
      grade: 'insufficient',
      label: 'Insufficient evidence',
      note: 'No comparable target results are available in the visible scope.',
      notes: [],
      coverageIssues,
      risks,
      minimumNextRun: `Run the same non-connectivity LLM pack, such as llm-reliability, against at least one local and one cloud target with ${recommendedTaskRepetitions} repetitions and 1 warmup.`,
    };
  }

  if (targetRows.length < 2) {
    return {
      tone: 'warn',
      grade: 'insufficient',
      label: 'Insufficient evidence',
      note: `Only one target is visible in this scope (${scope}); this can validate a target but cannot rank local vs cloud models.`,
      notes: ['Run the same pack against at least one local and one cloud target before treating the ranking as a comparison.'],
      coverageIssues,
      risks,
      minimumNextRun: `Run the same non-connectivity LLM pack, such as llm-reliability, against at least one local and one cloud target with ${recommendedTaskRepetitions} repetitions and 1 warmup.`,
    };
  }

  if (connectivityOnly || packEvidenceIssues.length || lowTaskRows || lowComparisonRows) {
    const reasons: string[] = [];
    if (connectivityOnly) {
      reasons.push('only the connectivity pack is in scope');
    }
    if (packEvidenceIssues.length) {
      reasons.push(`pack evidence warning(s): ${packEvidenceIssueSummary(packEvidenceIssues)}`);
    }
    if (lowTaskRows) {
      reasons.push(`${lowTaskRows}/${taskRows.length} task-target row(s) have fewer than ${recommendedTaskRepetitions} repetitions`);
    }
    if (lowComparisonRows) {
      reasons.push(`${lowComparisonRows}/${comparisonRows.length} comparison row(s) have fewer than ${recommendedTaskRepetitions} measured runs`);
    }
    if (costGapTargets.length) {
      reasons.push(`${costGapTargets.length} target(s) are missing cost metrics: ${costGapTargets.join(', ')}`);
    }
    if (pricingAssumptionTargets.length) {
      reasons.push(`${pricingAssumptionTargets.length} target(s) have pricing assumptions: ${pricingAssumptionTargets.join(', ')}`);
    }
    if (modelIdentityWarnings.length) {
      reasons.push(`${modelIdentityWarnings.length} comparison aggregate(s) have missing, fallback, or inconsistent served model ids`);
    }
    if (generationSettingWarnings.length) {
      reasons.push(`${generationSettingWarnings.length} generation setting warning(s) indicate mixed deterministic/exploratory sampling`);
    }
    if (packCalibrationIssues.length) {
      reasons.push(`pack calibration warning(s): ${packCalibrationIssueSummary(packCalibrationIssues)}`);
    }
    return {
      tone: 'warn',
      grade: 'smoke',
      label: 'Smoke evidence',
      note: `Smoke evidence: visible results prove the setup can run, but they are too shallow for model selection (${scope}; ${reasons.join('; ')}).`,
      notes: [
        targetCoverageParityNote(targetRows),
        overlapTargets.length
          ? `The leader's Wilson 95% pass-rate interval overlaps ${overlapTargets.length} contender(s): ${overlapTargets.join(', ')}.`
          : 'Resolve repetition depth before treating interval separation as decisive.',
      ],
      coverageIssues,
      risks,
      minimumNextRun: costGapTargets.length
        ? `Add pricing for targets with missing cost metrics (${costGapTargets.join(', ')}), then run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup.`
        : pricingAssumptionTargets.length
          ? `Add cache read/write pricing for targets with pricing assumptions (${pricingAssumptionTargets.join(', ')}), then rerun or re-export before using cost rankings as decisive evidence.`
        : modelIdentityWarnings.length
          ? `Confirm each target reports a stable provider-supplied served model id, split mixed served-model results into separate targets if needed, then run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup.`
        : generationSettingWarnings.length
            ? `Rerun or filter the same targets and pack with one shared generation policy, such as temperature 0, top_p 1, and a consistent seed policy, with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup.`
            : packEvidenceIssues.length
              ? `Run a prompt-comparison pack such as llm-reliability or llm-decision-suite against the same targets with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup, or strengthen the private pack scoring and task breadth.`
            : packCalibrationIssues.length
              ? `Calibrate or review the benchmark pack with baseline evidence, then rerun or filter the same targets with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup before selecting a winner.`
        : `Run a quality pack such as llm-reliability or llm-decision-suite against the same targets with at least ${recommendedTaskRepetitions} repetitions per task/target and 1 warmup.`,
    };
  }

  if (coverageIssues.length || !sameRunGroups || overlapTargets.length || costGapTargets.length || pricingAssumptionTargets.length || modelIdentityWarnings.length || generationSettingWarnings.length || packEvidenceIssues.length || packCalibrationIssues.length) {
    const reasons: string[] = [];
    if (coverageIssues.length) {
      reasons.push(`${coverageIssues.length} target(s) are missing visible pack/task slots`);
    }
    if (!sameRunGroups) {
      reasons.push('targets were not compared in the same run groups');
    }
    if (overlapTargets.length) {
      reasons.push(`the leader's Wilson 95% pass-rate interval overlaps ${overlapTargets.length} target(s)`);
    }
    if (costGapTargets.length) {
      reasons.push(`${costGapTargets.length} target(s) are missing cost metrics: ${costGapTargets.join(', ')}`);
    }
    if (pricingAssumptionTargets.length) {
      reasons.push(`${pricingAssumptionTargets.length} target(s) have pricing assumptions: ${pricingAssumptionTargets.join(', ')}`);
    }
    if (modelIdentityWarnings.length) {
      reasons.push(`${modelIdentityWarnings.length} comparison aggregate(s) have missing, fallback, or inconsistent served model ids`);
    }
    if (generationSettingWarnings.length) {
      reasons.push(`${generationSettingWarnings.length} generation setting warning(s) indicate mixed deterministic/exploratory sampling`);
    }
    if (packEvidenceIssues.length) {
      reasons.push(`pack evidence warning(s): ${packEvidenceIssueSummary(packEvidenceIssues)}`);
    }
    if (packCalibrationIssues.length) {
      reasons.push(`pack calibration warning(s): ${packCalibrationIssueSummary(packCalibrationIssues)}`);
    }
    return {
      tone: 'warn',
      grade: 'directional',
      label: 'Directional evidence',
      note: `Directional evidence: sample depth is usable, but the ranking is not yet decisive because ${reasons.join('; ')} (${scope}).`,
      notes: [targetCoverageParityNote(targetRows), targetConfidenceNote(targetRows, comparisonRows, taskRows)],
      coverageIssues,
      risks,
      minimumNextRun: costGapTargets.length
        ? 'Add pricing for targets with missing cost metrics, then re-run the same targets and pack so cost can be compared beside quality and latency.'
        : pricingAssumptionTargets.length
          ? 'Add cache read/write pricing for targets with pricing assumptions, then re-run or re-export before treating cost ranking as decisive.'
        : coverageIssues.length
          ? `Run the missing pack/task slots for every target with at least ${recommendedTaskRepetitions} repetitions per task/target.`
          : !sameRunGroups
            ? `Re-run all compared targets together on the same pack with at least ${recommendedTaskRepetitions} repetitions per task/target.`
          : modelIdentityWarnings.length
            ? 'Confirm each target reports a stable provider-supplied served model id, split mixed served-model results into separate targets if needed, then re-run the same targets and pack.'
          : generationSettingWarnings.length
              ? 'Rerun or filter the same targets and pack with one shared generation policy, such as temperature 0, top_p 1, and a consistent seed policy.'
              : packEvidenceIssues.length
                ? 'Run a prompt-comparison pack such as llm-reliability or llm-decision-suite against the same targets before treating this ranking as model-selection evidence.'
              : packCalibrationIssues.length
                ? 'Calibrate or review the benchmark pack with documented baseline runs before treating this ranking as a model-selection decision.'
          : 'Increase repetitions or add more discriminating tasks until the leader\'s Wilson interval separates from contenders.',
    };
  }

  return {
    tone: 'ok',
    grade: 'comparison_ready',
    label: 'Comparison-ready',
    note: `Comparison-ready evidence: targets share pack/task coverage, run groups, cost coverage, stable served-model identity, calibrated pack metadata, and one generation policy; every task-target row has at least ${recommendedTaskRepetitions} repetitions, and the leader's Wilson interval is separated (${scope}).`,
    notes: [targetCoverageParityNote(targetRows), targetConfidenceNote(targetRows, comparisonRows, taskRows)],
    coverageIssues,
    risks,
    minimumNextRun: 'No immediate rerun is required for a first-pass comparison; add domain-specific packs before final production selection.',
  };
}

function evidenceDecisionStatus(evidence: ComparisonEvidenceAssessment): ModelSelectionDecisionStatus {
  if (evidence.grade === 'comparison_ready') {
    return 'select_recommended_target';
  }
  if (evidence.grade === 'insufficient') {
    return 'insufficient_evidence';
  }
  return 'collect_more_evidence';
}

function evidenceSelectedTargetId(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[]) {
  return evidence.grade === 'comparison_ready' ? targetRows[0]?.targetId ?? null : null;
}

function evidenceSelectionNote(evidence: ComparisonEvidenceAssessment, targetRows: TargetRankingRow[]) {
  if (evidence.grade === 'comparison_ready') {
    const targetId = targetRows[0]?.targetId ?? 'the recommended target';
    return `Evidence is comparison-ready; select ${targetId} for this result scope unless external domain constraints override it.`;
  }
  if (evidence.grade === 'insufficient') {
    return `Do not select a winner yet; ${evidence.minimumNextRun}`;
  }
  return `Collect more evidence before choosing a winner; ${evidence.minimumNextRun}`;
}

function targetCoverageIssues(rows: TargetRankingRow[]): ResultCoverageIssue[] {
  if (rows.length < 2) {
    return [];
  }
  const allPackIds = unionStrings(rows.flatMap(row => row.packIds));
  const allTaskIds = unionStrings(rows.flatMap(row => row.taskIds));
  const allSlots = unionStrings(rows.flatMap(row => row.packTaskSlots));
  return rows.map(row => ({
    targetId: row.targetId,
    missingPackTaskSlots: differenceStrings(allSlots, row.packTaskSlots),
    missingPacks: differenceStrings(allPackIds, row.packIds),
    missingTasks: differenceStrings(allTaskIds, row.taskIds),
  }))
    .filter(issue => issue.missingPackTaskSlots.length || issue.missingPacks.length || issue.missingTasks.length)
    .sort((a, b) => (b.missingPackTaskSlots.length + b.missingTasks.length + b.missingPacks.length) - (a.missingPackTaskSlots.length + a.missingTasks.length + a.missingPacks.length)
      || a.targetId.localeCompare(b.targetId));
}

function targetCostCoverageGapIds(rows: TargetRankingRow[]) {
  return rows
    .filter(row => row.runs > 0 && row.costedRuns < row.runs)
    .map(row => row.targetId)
    .sort();
}

function targetPricingAssumptionIds(rows: TargetRankingRow[]) {
  return rows
    .filter(row => row.pricingAssumptionRuns > 0)
    .map(row => row.targetId)
    .sort();
}

function targetsCoverSameRunGroups(rows: TargetRankingRow[]) {
  const firstGroups = rows[0]?.groupIds ?? [];
  return rows.every(row => sameStringArray(row.groupIds, firstGroups));
}

function comparisonScopeIsConnectivityOnly(rows: TargetRankingRow[]) {
  const packIds = unionStrings(rows.flatMap(row => row.packIds));
  return packIds.length > 0 && packIds.every(packId => packId === 'llm-connectivity');
}

function unionStrings(values: string[]) {
  return Array.from(new Set(values)).sort();
}

function differenceStrings(allValues: string[], presentValues: string[]) {
  const present = new Set(presentValues);
  return allValues.filter(value => !present.has(value));
}

function sameStringArray(a: string[], b: string[]) {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

function packTaskSlotId(packId: string, taskId: string) {
  return `${packId}/${taskId}`;
}

function previewList(values: string[], limit = 3) {
  if (!values.length) {
    return '-';
  }
  const visible = values.slice(0, limit).join(', ');
  const remaining = values.length - limit;
  return remaining > 0 ? `${visible}, +${remaining} more` : visible;
}

function passRateInterval(passed: number, runs: number) {
  if (runs <= 0) {
    return null;
  }
  const z = 1.96;
  const p = passed / runs;
  const denominator = 1 + (z * z) / runs;
  const center = (p + (z * z) / (2 * runs)) / denominator;
  const margin = (z * Math.sqrt((p * (1 - p)) / runs + (z * z) / (4 * runs * runs))) / denominator;
  return {
    low: Math.max(0, center - margin),
    high: Math.min(1, center + margin),
  };
}

function compareDecisionRows(a: ComparisonRow, b: ComparisonRow) {
  return b.passRate - a.passRate
    || nullNumberDesc(a.avgScore, b.avgScore)
    || nullNumberAsc(a.p95TimeMs ?? a.avgTimeMs, b.p95TimeMs ?? b.avgTimeMs)
    || nullNumberAsc(a.avgCostUsd, b.avgCostUsd)
    || nullNumberDesc(a.avgOutputTokensPerSecond, b.avgOutputTokensPerSecond)
    || b.runs - a.runs
    || a.targetId.localeCompare(b.targetId);
}

function minBy<T>(items: T[], score: (item: T) => number): T | undefined {
  return items.reduce<T | undefined>((best, item) => {
    if (!best || score(item) < score(best)) {
      return item;
    }
    return best;
  }, undefined);
}

function maxBy<T>(items: T[], score: (item: T) => number): T | undefined {
  return items.reduce<T | undefined>((best, item) => {
    if (!best || score(item) > score(best)) {
      return item;
    }
    return best;
  }, undefined);
}

function nullNumberAsc(a: number | null | undefined, b: number | null | undefined) {
  const left = a ?? Number.POSITIVE_INFINITY;
  const right = b ?? Number.POSITIVE_INFINITY;
  return left - right;
}

function nullNumberDesc(a: number | null | undefined, b: number | null | undefined) {
  const left = a ?? Number.NEGATIVE_INFINITY;
  const right = b ?? Number.NEGATIVE_INFINITY;
  return right - left;
}

function buildResultSummary(results: RunResult[]) {
  const scores = results.map(result => result.score).filter((value): value is number => typeof value === 'number');
  const times = results.map(resultWallTimeMs).filter((value): value is number => typeof value === 'number');
  const throughputs = results.map(result => result.output_tokens_per_second).filter((value): value is number => typeof value === 'number');
  const totalTokensValue = results.reduce((sum, result) => sum + (totalTokens(result) ?? 0), 0);
  const totalCost = results.reduce((sum, result) => sum + (resultCostUsdForCoverage(result) ?? 0), 0);
  const passed = results.filter(result => result.status === 'passed').length;
  return {
    total: results.length,
    passed,
    failed: results.length - passed,
    passRate: results.length ? passed / results.length : 0,
    avgScore: average(scores),
    avgOutputTokensPerSecond: average(throughputs),
    p95TimeMs: percentile(times, 0.95),
    totalTokens: totalTokensValue,
    totalCostUsd: results.some(resultHasCostCoverage) ? totalCost : null,
  };
}

function buildMetricCoverageRows(results: RunResult[]): MetricCoverageRow[] {
  return [
    metricCoverageRow(results, 'Score', result => typeof result.score === 'number', 'Missing when a run fails before scoring completes.'),
    metricCoverageRow(results, 'pass_fail', result => typeof result.pass_fail === 'boolean', 'Required v1 alias derived from run status.'),
    metricCoverageRow(results, 'score_numeric', result => typeof result.score_numeric === 'number', 'Required v1 alias for score.'),
    metricCoverageRow(results, 'Wall time', result => typeof resultWallTimeMs(result) === 'number', 'Expected for persisted runs; missing means timing was not stored.'),
    metricCoverageRow(results, 'Setup time', result => typeof result.setup_time_ms === 'number', 'Prompt and repo/code tasks report app/workspace setup time before target execution.'),
    metricCoverageRow(results, 'Target time', result => typeof result.target_time_ms === 'number', 'Prompt and repo/code tasks report time spent invoking the benchmark target before evaluation.'),
    metricCoverageRow(results, 'Evaluation time', result => typeof result.evaluation_time_ms === 'number', 'Scoring and repo/code tasks report time spent in the evaluation command after target execution.'),
    metricCoverageRow(results, 'Model call time', result => typeof result.model_call_wall_time_ms === 'number', 'Provider-backed repo/code tasks report the model invocation wall time separately from scoring time.'),
    metricCoverageRow(results, 'Exit code', result => typeof result.exit_code === 'number', 'Process-backed scoring runs report the normalized scoring command exit code.'),
    metricCoverageRow(results, 'Harness exit code', result => typeof result.harness_exit_code === 'number', 'Worker harness command runs report the external harness process exit code when available.'),
    metricCoverageRow(results, 'Stdout bytes', result => typeof result.stdout_bytes === 'number', 'Process-backed runs report redacted stdout byte counts for artifact sizing and debugging.'),
    metricCoverageRow(results, 'Stderr bytes', result => typeof result.stderr_bytes === 'number', 'Process-backed runs report redacted stderr byte counts for artifact sizing and debugging.'),
    metricCoverageRow(results, 'Files changed', result => typeof result.files_changed === 'number', 'Repo/code tasks report how many files changed in the captured git diff.'),
    metricCoverageRow(results, 'Lines added', result => typeof result.lines_added === 'number', 'Repo/code tasks report added lines from the captured git diff.'),
    metricCoverageRow(results, 'Lines deleted', result => typeof result.lines_deleted === 'number', 'Repo/code tasks report deleted lines from the captured git diff.'),
    metricCoverageRow(results, 'Commands observed', result => typeof result.commands_observed_count === 'number', 'Process-backed repo/code and worker harness runs report benchmark commands BenchForge observed or executed.'),
    metricCoverageRow(results, 'Dangerous command hits', result => typeof result.dangerous_command_hits === 'number', 'Repo/code tasks count suspicious command patterns detected in redacted stdout and stderr.'),
    metricCoverageRow(results, 'Provider TTFB', result => typeof result.provider_time_to_first_byte_ms === 'number', 'Only provider-backed model calls report transport timing.'),
    metricCoverageRow(results, 'TTFT', result => typeof result.provider_time_to_first_token_ms === 'number', 'Only streaming provider calls report time to first token.'),
    metricCoverageRow(results, 'ttft_ms', result => typeof result.ttft_ms === 'number', 'Required v1 alias for time to first token.'),
    metricCoverageRow(results, 'Provider total', result => typeof result.provider_request_total_ms === 'number', 'Recorded when adapter calls expose request timing.'),
    metricCoverageRow(results, 'Prompt tokens', result => typeof result.prompt_tokens === 'number', 'Requires provider token usage or a local runtime that reports prompt tokens.'),
    metricCoverageRow(results, 'input_tokens', result => typeof result.input_tokens === 'number', 'Required v1 alias for prompt/input tokens.'),
    metricCoverageRow(results, 'Completion tokens', result => typeof result.completion_tokens === 'number', 'Requires provider token usage or a local runtime that reports output tokens.'),
    metricCoverageRow(results, 'output_tokens', result => typeof result.output_tokens === 'number', 'Required v1 alias for completion/output tokens.'),
    metricCoverageRow(results, 'Reasoning tokens', result => typeof result.reasoning_tokens === 'number', 'Only reasoning-capable providers/models report this metric.'),
    metricCoverageRow(results, 'Cached tokens', result => typeof result.cached_tokens === 'number', 'Providers with prompt cache accounting report cached input tokens when available.'),
    metricCoverageRow(results, 'Cache read tokens', result => typeof result.cache_read_tokens === 'number', 'Providers with prompt cache accounting report cache-read input tokens when available.'),
    metricCoverageRow(results, 'Cache write tokens', result => typeof result.cache_write_tokens === 'number', 'Providers with prompt cache accounting report cache-write or cache-creation input tokens when available.'),
    metricCoverageRow(results, 'Total tokens', result => totalTokens(result) != null, 'Uses provider total tokens when available or prompt plus completion tokens when both are present.'),
    metricCoverageRow(results, 'Output tok/s', result => typeof result.output_tokens_per_second === 'number', 'Requires completion token counts and wall time.'),
    metricCoverageRow(results, 'decode_tokens_per_sec', result => typeof result.decode_tokens_per_sec === 'number', 'Required v1 alias for output token throughput.'),
    metricCoverageRow(results, 'Peak RSS', result => typeof result.peak_rss_mb === 'number', 'Process-backed runs report peak resident memory only when BenchForge or a worker can observe it.'),
    metricCoverageRow(results, 'HTTP status', result => typeof result.http_status === 'number', 'Only HTTP provider calls expose this.'),
    metricCoverageRow(results, 'Retry attempts', result => typeof result.provider_attempts === 'number', 'Only retry-aware provider calls expose attempt counts.'),
    metricCoverageRow(results, 'Retry-After', result => typeof result.provider_retry_after_ms === 'number', 'Only provider responses with Retry-After headers expose this.'),
    metricCoverageRow(results, 'Retry delay', result => typeof result.provider_retry_delay_ms === 'number', 'Recorded when BenchForge waits before retrying a provider call.'),
    metricCoverageRow(results, 'Provider model', result => nonEmptyString(result.provider_model), 'Provider-supplied when available; local runtimes may be confirmed from /models before BenchForge falls back to the configured target model.'),
    metricCoverageRow(results, 'Provider model source', result => nonEmptyString(result.provider_model_source), 'Identifies whether provider_model came from the provider response, a local runtime model list, or the configured target model.'),
    metricCoverageRow(results, 'Finish reason', result => nonEmptyString(result.finish_reason), 'Only model APIs that report completion finish reasons expose this.'),
    metricCoverageRow(results, 'Cost', resultHasCostCoverage, 'Requires token usage plus configured pricing, or a known-zero local/mock target.'),
    metricCoverageRow(results, 'estimated_cost_usd', result => typeof result.estimated_cost_usd === 'number', 'Required v1 alias for estimated benchmark cost.'),
    metricCoverageRow(results, 'Pricing assumption', result => nonEmptyString(result.pricing_assumption), 'Present when a cost estimate used a documented pricing fallback, such as prompt-cache tokens priced at normal input-token rates.'),
    metricCoverageRow(results, 'Safety findings', result => typeof result.security_finding_count === 'number', 'Worker security packs report finding counts as first-class result metrics.'),
    metricCoverageRow(results, 'Safety files', result => typeof result.security_files_scanned === 'number', 'Worker security packs report how many files or manifests were inspected.'),
    metricCoverageRow(results, 'Import format', result => nonEmptyString(result.import_format), 'Worker harness imports set this when a run was read from external result files.'),
    metricCoverageRow(results, 'Import source', result => nonEmptyString(result.import_source), 'Identifies whether imported harness output came from a file, directory, or other supported path.'),
    metricCoverageRow(results, 'Import files', result => typeof result.import_file_count === 'number', 'Counts how many imported result files contributed to the run result.'),
    metricCoverageRow(results, 'Import total files', result => typeof result.import_total_file_count === 'number', 'Counts all supported result files discovered before import limits were applied.'),
    metricCoverageRow(results, 'Import omitted files', result => typeof result.import_omitted_file_count === 'number', 'Counts supported result files skipped after worker import limits were reached.'),
    metricCoverageRow(results, 'Import unsupported files', result => typeof result.import_unsupported_file_count === 'number', 'Counts unsupported side files ignored during worker directory imports.'),
    metricCoverageRow(results, 'Import truncated', result => typeof result.import_truncated === 'number', 'Set by worker imports to show whether imported result evidence was truncated or partially bounded.'),
    metricCoverageRow(results, 'Import truncated bytes', result => typeof result.import_truncated_bytes === 'number', 'Counts bytes omitted from imported result evidence when import size limits apply.'),
    metricCoverageRow(results, 'Summary parser', result => nonEmptyString(result.summary_source), 'Identifies the parser that extracted pass/fail summary from imported harness output.'),
  ];
}

function metricCoverageRow(results: RunResult[], label: string, predicate: (result: RunResult) => boolean, note: string): MetricCoverageRow {
  const present = results.filter(predicate).length;
  return {
    label,
    present,
    missing: results.length - present,
    note,
  };
}

function nonEmptyString(value: unknown) {
  return typeof value === 'string' && value.trim().length > 0;
}

function formatImportProvenance(result: RunResult) {
  const parts: string[] = [];
  if (nonEmptyString(result.import_format)) {
    parts.push(result.import_format!.trim());
  }
  if (nonEmptyString(result.import_source)) {
    parts.push(result.import_source!.trim());
  }
  if (typeof result.import_file_count === 'number') {
    const count = Math.round(result.import_file_count);
    if (typeof result.import_total_file_count === 'number') {
      const total = Math.round(result.import_total_file_count);
      parts.push(`${count}/${total} files`);
    } else {
      parts.push(`${count} file${count === 1 ? '' : 's'}`);
    }
  }
  if (typeof result.import_truncated === 'number' && result.import_truncated > 0) {
    parts.push('partial');
  }
  if (typeof result.import_omitted_file_count === 'number' && result.import_omitted_file_count > 0) {
    const omitted = Math.round(result.import_omitted_file_count);
    parts.push(`${omitted} omitted`);
  }
  if (typeof result.import_unsupported_file_count === 'number' && result.import_unsupported_file_count > 0) {
    const unsupported = Math.round(result.import_unsupported_file_count);
    parts.push(`${unsupported} unsupported`);
  }
  if (typeof result.import_truncated_bytes === 'number' && result.import_truncated_bytes > 0) {
    const bytes = Math.round(result.import_truncated_bytes);
    parts.push(`${bytes} bytes truncated`);
  }
  return parts.length ? parts.join(' / ') : '-';
}

function ResultErrorCell({ result }: { result: RunResult }) {
  const code = resultErrorCode(result);
  const detail = resultErrorDetail(result);
  if (!code && !detail) {
    return <span className="muted">-</span>;
  }
  const title = [code, result.error_message ?? result.error].filter(Boolean).join(': ');
  return <span title={title || undefined}><strong>{code || 'error'}</strong>{detail ? <span className="muted result-error-detail">{truncateErrorDetail(detail)}</span> : null}</span>;
}

function resultErrorCode(result: RunResult) {
  if (result.status === 'passed') {
    return '';
  }
  return result.error_code || result.status || 'unknown';
}

function resultErrorDetail(result: RunResult) {
  return (result.error_message || result.error || '').trim();
}

function truncateErrorDetail(detail: string, limit = 110) {
  const normalized = detail.replace(/\s+/g, ' ').trim();
  if (normalized.length <= limit) {
    return normalized;
  }
  return `${normalized.slice(0, limit - 3).trimEnd()}...`;
}

function buildErrorRows(results: RunResult[]) {
  const rows = new Map<string, { code: string; count: number; examples: string[] }>();
  for (const result of results) {
    if (result.status === 'passed') {
      continue;
    }
    const code = resultErrorCode(result) || 'unknown';
    const row = rows.get(code) ?? { code, count: 0, examples: [] };
    row.count += 1;
    const detail = resultErrorDetail(result);
    const example = detail ? truncateErrorDetail(detail, 86) : '';
    if (example && !row.examples.includes(example) && row.examples.length < 3) {
      row.examples.push(example);
    }
    rows.set(code, row);
  }
  return Array.from(rows.values()).sort((a, b) => b.count - a.count || a.code.localeCompare(b.code));
}

function buildErrorRecoveryRows(results: RunResult[]): ErrorRecoveryRow[] {
  type MutableErrorRecoveryRow = ErrorRecoveryRow & {
    packSet: Set<string>;
    targetSet: Set<string>;
    taskSet: Set<string>;
    httpStatusCounts: Map<number, number>;
  };
  const rows = new Map<string, MutableErrorRecoveryRow>();
  for (const result of results) {
    if (result.status === 'passed') {
      continue;
    }
    const code = resultErrorCode(result) || 'unknown';
    const packId = resultPackId(result);
    const key = `${code}\u0000${packId}`;
    let row = rows.get(key);
    if (!row) {
      row = {
        key,
        code,
        count: 0,
        packIds: [],
        targetIds: [],
        taskIds: [],
        httpStatuses: '-',
        retryable: errorCategoryIsRetryable(code),
        recoveryHint: errorCategoryRecoveryHint(code),
        exampleDetail: '',
        packSet: new Set(),
        targetSet: new Set(),
        taskSet: new Set(),
        httpStatusCounts: new Map(),
      };
      rows.set(key, row);
    }
    row.count += 1;
    row.packSet.add(packId);
    row.targetSet.add(resultTargetId(result));
    row.taskSet.add(resultTaskId(result));
    if (typeof result.http_status === 'number' && Number.isFinite(result.http_status)) {
      const status = Math.round(result.http_status);
      row.httpStatusCounts.set(status, (row.httpStatusCounts.get(status) ?? 0) + 1);
    }
    const detail = resultErrorDetail(result);
    if (detail && !row.exampleDetail) {
      row.exampleDetail = truncateErrorDetail(detail, 140);
    }
  }
  return Array.from(rows.values())
    .map(row => ({
      key: row.key,
      code: row.code,
      count: row.count,
      packIds: Array.from(row.packSet).sort((a, b) => a.localeCompare(b)),
      targetIds: Array.from(row.targetSet).sort((a, b) => a.localeCompare(b)),
      taskIds: Array.from(row.taskSet).sort((a, b) => a.localeCompare(b)),
      httpStatuses: formatHttpStatusCounts(row.httpStatusCounts),
      retryable: row.retryable,
      recoveryHint: row.recoveryHint,
      exampleDetail: row.exampleDetail,
    }))
    .sort((a, b) => b.count - a.count || a.code.localeCompare(b.code) || previewList(a.packIds).localeCompare(previewList(b.packIds)));
}

function errorCategoryIsRetryable(code: string) {
  return ['endpoint_unreachable', 'rate_limit', 'timeout', 'network', 'server_error', 'provider_failed', 'failed'].includes(code);
}

function errorCategoryHasTargetRepair(code: string) {
  return ['missing_key', 'auth', 'model_not_found', 'endpoint_unreachable', 'timeout', 'unsupported_shape', 'malformed_response'].includes(code);
}

function errorCategoryRepairHint(code: string) {
  switch (code) {
    case 'missing_key':
      return 'Open Targets to save the provider key or configure the expected key source, then validate the target before rerunning.';
    case 'auth':
      return 'Open Targets to replace or re-check the provider key and account/model access, then validate the target before rerunning.';
    case 'model_not_found':
      return 'Open Targets to refresh the model id or choose another model, then validate the target before rerunning.';
    case 'endpoint_unreachable':
      return 'Open Targets to check the base URL, port, runtime status, VPN, or firewall, then validate the target before rerunning.';
    case 'timeout':
      return 'Open Targets to increase the target timeout or reduce max output tokens, then validate the target before rerunning.';
    case 'unsupported_shape':
      return 'Open Targets to check the adapter and endpoint type, then validate the target before rerunning.';
    case 'malformed_response':
      return 'Open Targets to check the adapter, endpoint, and model output format; inspect artifacts if the target still validates.';
    case 'pricing_assumption':
      return 'Open Targets to add missing input/output or prompt-cache pricing for the affected target, then validate and rerun comparison evidence.';
    default:
      return 'Open Targets to repair the affected target, then validate before rerunning.';
  }
}

function errorCategoryRecoveryHint(code: string) {
  switch (code) {
    case 'missing_key':
      return 'Add the provider API key in Settings or export the required environment variable, then revalidate the target.';
    case 'auth':
      return 'Check that the saved key is valid for this provider, model, and organization, then revalidate the target.';
    case 'model_not_found':
      return 'Confirm the model id is available to the provider account, refresh catalog search, or choose another model.';
    case 'endpoint_unreachable':
      return 'Start the local server or verify the base URL, port, VPN, and firewall before retrying.';
    case 'rate_limit':
      return 'Wait for the quota window to reset, lower concurrency, or raise the provider quota before retrying.';
    case 'timeout':
      return 'Increase the target timeout, reduce max output tokens, lower concurrency, or retry with a smaller pack.';
    case 'network':
      return 'Check network connectivity, proxy settings, TLS interception, and provider status before retrying.';
    case 'server_error':
      return 'Retry after the provider recovers; repeated failures should be validated against a smaller smoke pack.';
    case 'provider_failed':
      return 'Review the provider response and retry after checking quota, endpoint health, and model availability.';
    case 'context_overflow':
      return 'Reduce prompt/context size or select a model with a larger context window.';
    case 'content_filter':
      return 'Review the task prompt and provider safety policy; adjust the benchmark task if the refusal is expected.';
    case 'malformed_response':
      return 'Inspect raw response artifacts and target output shape; tighten JSON/format instructions or fix adapter parsing.';
    case 'unsupported_shape':
      return 'Update the target adapter or benchmark task to match the response schema BenchForge expects.';
    case 'security_findings':
      return 'Inspect worker security artifacts and fix the reported dependency or command findings before rerunning.';
    case 'cancelled':
      return 'Rerun the job if cancellation was accidental; partial results remain usable as scoped evidence.';
    case 'test_failed':
      return 'Open stdout, stderr, diff, and scorer artifacts to determine whether the model failed the benchmark task.';
    case 'score_failed':
      return 'Inspect scorer output and task configuration; fix scorer dependencies or malformed expected-output data.';
    case 'failed':
      return 'Review the run artifacts for the underlying failure, then retry after addressing the first actionable error.';
    default:
      return 'Inspect artifacts and target validation details, then rerun a small smoke pack after the cause is fixed.';
  }
}

function buildScoreDistributionRows(results: RunResult[]): ChartRow[] {
  const buckets = [
    { label: '1.00', min: 0.999999, max: 1, tone: 'ok' },
    { label: '0.75-0.99', min: 0.75, max: 0.999998999, tone: 'ok' },
    { label: '0.50-0.74', min: 0.5, max: 0.749999, tone: 'warn' },
    { label: '0.01-0.49', min: 0.000001, max: 0.499999, tone: 'error' },
    { label: '0.00', min: 0, max: 0, tone: 'error' },
  ];
  const counts = buckets.map(bucket => ({ ...bucket, count: 0 }));
  for (const result of results) {
    const score = result.score;
    if (typeof score !== 'number' || !Number.isFinite(score)) {
      continue;
    }
    const clamped = Math.max(0, Math.min(1, score));
    const bucket = counts.find(candidate => clamped >= candidate.min && clamped <= candidate.max);
    if (bucket) {
      bucket.count += 1;
    }
  }
  return counts
    .filter(bucket => bucket.count > 0)
    .map(bucket => ({
      label: bucket.label,
      value: bucket.count,
      valueLabel: String(bucket.count),
      tone: bucket.tone,
    }));
}

function uniqueSorted(values: string[]) {
  return Array.from(new Set(values.filter(Boolean))).sort((a, b) => a.localeCompare(b));
}

function validationSummary(results: TargetValidation[]) {
  const ok = results.filter(result => result.status === 'ok').length;
  const detail = formatValidationCodeCounts(results.filter(result => result.status !== 'ok'));
  return detail ? `${ok}/${results.length} targets ok; ${detail}` : `${ok}/${results.length} targets ok`;
}

function formatValidationCodeCounts(results: TargetValidation[]) {
  const nonOkCodes = new Map<string, number>();
  for (const result of results) {
    const code = validationDetailCode(result.detail);
    nonOkCodes.set(code, (nonOkCodes.get(code) ?? 0) + 1);
  }
  return Array.from(nonOkCodes.entries())
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([code, count]) => `${code} ${count}`)
    .join(', ');
}

function validationDetailCode(detail: string) {
  const match = detail.match(/^([a-z_]+):/);
  return match?.[1] ?? 'needs_review';
}

function validationRepairCode(result: TargetValidation) {
  const code = validationDetailCode(result.detail);
  if (code === 'rate_limited') {
    return 'rate_limit';
  }
  return code;
}

function resultScopeFilters({
  statusFilter,
  packFilter,
  targetFilter,
  groupFilter,
  providerFilter,
  providerModelFilter,
  dateWindow,
  providerOptions,
  providerModelOptions,
}: {
  statusFilter: string;
  packFilter: string;
  targetFilter: string;
  groupFilter: string;
  providerFilter: string;
  providerModelFilter: string;
  dateWindow: DateWindow;
  providerOptions: Array<{ key: string; label: string }>;
  providerModelOptions: Array<{ key: string; label: string }>;
}) {
  const filters: string[] = [];
  if (packFilter !== 'all') {
    filters.push(`pack ${packFilter}`);
  }
  if (targetFilter !== 'all') {
    filters.push(`target ${targetFilter}`);
  }
  if (groupFilter !== 'all') {
    filters.push(`group ${groupFilter.slice(0, 8)}`);
  }
  if (providerFilter !== 'all') {
    const label = providerOptions.find(provider => provider.key === providerFilter)?.label ?? providerFilter;
    filters.push(`provider ${label}`);
  }
  if (providerModelFilter !== 'all') {
    const label = providerModelOptions.find(model => model.key === providerModelFilter)?.label ?? providerModelFilter;
    filters.push(`model ${label}`);
  }
  if (statusFilter !== 'all') {
    filters.push(`status ${statusFilter}`);
  }
  if (dateWindow !== 'all') {
    filters.push(`date ${dateWindowLabel(dateWindow)}`);
  }
  return filters;
}

function resultScopeWarning(activeFilters: string[], visibleCount: number, totalCount: number, statusFilter: string, targetFilter: string) {
  if (!activeFilters.length) {
    return '';
  }
  const notes = [
    `Results, charts, rankings, recommendations, and exports use the filtered scope: ${activeFilters.join('; ')} (${visibleCount}/${totalCount} result rows).`,
    'Clear filters before treating pass rates, costs, or model rankings as whole-history evidence.',
  ];
  if (statusFilter !== 'all') {
    notes.push('Status filters can hide failures or successes and make pass rates look better or worse than the full run group.');
  }
  if (targetFilter !== 'all') {
    notes.push('A single-target scope cannot prove a model won against hidden targets.');
  }
  return notes.join(' ');
}

function buildProviderOptions(results: RunResult[], adapterById: Map<string, Adapter>) {
  const labels = new Map<string, string>();
  for (const result of results) {
    const key = resultProviderKey(result);
    labels.set(key, resultProviderLabel(result, adapterById));
  }
  return Array.from(labels, ([key, label]) => ({ key, label }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

function buildProviderModelOptions(results: RunResult[]) {
  const labels = new Map<string, string>();
  for (const result of results) {
    const key = resultProviderModelKey(result);
    labels.set(key, key === missingProviderModelKey ? 'Not reported' : key);
  }
  return Array.from(labels, ([key, label]) => ({ key, label }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

function resultPackId(result: RunResult) {
  return result.benchmarkPackId ?? result.benchmark_pack_id ?? '-';
}

function resultTargetId(result: RunResult) {
  return result.targetId ?? result.target_id ?? '-';
}

function resultGroupId(result: RunResult) {
  return result.run_group_id || result.id;
}

function resultTaskId(result: RunResult) {
  return result.taskId ?? result.task_id ?? '-';
}

function resultTaskWeight(result: RunResult) {
  const task = result.reproducibility?.task;
  if (task && typeof task === 'object' && !Array.isArray(task) && 'weight' in task) {
    const weight = (task as { weight?: unknown }).weight;
    if (typeof weight === 'number' && Number.isFinite(weight) && weight > 0) {
      return weight;
    }
  }
  return 1;
}

function resultWallTimeMs(result: RunResult) {
  return result.wallTimeMs ?? result.wall_time_ms ?? null;
}

function resultProviderModelKey(result: RunResult) {
  const model = result.provider_model?.trim();
  return model || missingProviderModelKey;
}

function resultProviderKey(result: RunResult) {
  return resultAdapterId(result) || missingProviderKey;
}

function resultProviderLabel(result: RunResult, adapterById: Map<string, Adapter>) {
  const adapterId = resultAdapterId(result);
  if (!adapterId) {
    return 'Not reported';
  }
  return adapterProviderLabel(adapterById.get(adapterId), adapterId);
}

function resultAdapterId(result: RunResult) {
  const target = result.reproducibility?.target;
  if (isRecord(target)) {
    const adapter = target.adapter_id ?? target.adapterId;
    if (typeof adapter === 'string' && adapter.trim()) {
      return adapter.trim();
    }
  }
  return null;
}

function adapterProviderLabel(adapter: Adapter | undefined, adapterId: string) {
  const metadataProvider = adapter?.metadata?.provider;
  if (typeof metadataProvider === 'string' && metadataProvider.trim()) {
    return metadataProvider.trim();
  }
  const fallback: Record<string, string> = {
    openai: 'OpenAI',
    anthropic: 'Anthropic',
    mistral: 'Mistral',
    openrouter: 'OpenRouter',
    'azure-openai': 'Azure OpenAI',
    gemini: 'Google Gemini',
    'ollama-openai': 'Ollama',
    'llama-cpp-openai': 'llama.cpp',
    'lm-studio-openai': 'LM Studio',
    'vllm-openai': 'vLLM',
    'mlx-lm': 'MLX',
    'omlx-experimental': 'oMLX',
    'generic-openai-compatible': 'OpenAI-compatible',
    'openai-compatible': 'OpenAI-compatible',
    mock: 'Mock',
  };
  return fallback[adapterId] ?? adapter?.name ?? adapterId;
}

const localModelAdapters = new Set([
  'llama-cpp-openai',
  'lm-studio-openai',
  'mlx-lm',
  'ollama-openai',
  'omlx-experimental',
  'vllm-openai',
]);

const cloudModelAdapters = new Set([
  'anthropic',
  'azure-openai',
  'gemini',
  'mistral',
  'openai',
  'openrouter',
]);

function isLocalModelTarget(target: Target) {
  if (target.isLocalModel != null || target.isCloudModel != null) {
    return target.isLocalModel === true;
  }
  return localModelAdapters.has(target.adapterId);
}

function isCloudModelTarget(target: Target) {
  if (target.isLocalModel != null || target.isCloudModel != null) {
    return target.isCloudModel === true;
  }
  return cloudModelAdapters.has(target.adapterId);
}

function targetValidationFromTarget(target: Target): TargetValidation | undefined {
  if (!target.validationStatus) {
    return undefined;
  }
  return {
    targetId: target.id,
    status: target.validationStatus,
    detail: target.validationDetail ?? '',
    checkedAt: target.validationCheckedAt ?? '',
  };
}

function targetEndpointDisplay(target: Target) {
  return target.endpoint || target.command || '';
}

function targetIsSelectableForRun(target: Target) {
  return target.enabled !== false && target.status !== 'invalid' && target.validationStatus !== 'error';
}

function targetIsSelectableModel(target: Target) {
  return targetIsSelectableForRun(target)
    && (target.kind === 'direct_model' || target.kind === 'harnessed_model');
}

function targetRepairTargetCanEdit(target: Target) {
  return target.enabled !== false && (target.kind === 'direct_model' || target.kind === 'benchmark_harness');
}

function errorRecoveryTargetAvailability(row: ErrorRecoveryRow, targets: Target[]): ErrorRecoveryTargetAvailability {
  const targetById = new Map(targets.map(target => [target.id, target]));
  const selectableTargetIds: string[] = [];
  const unavailableTargetIds: string[] = [];
  const missingTargetIds: string[] = [];
  for (const targetId of row.targetIds.filter(id => id && id !== '-')) {
    const target = targetById.get(targetId);
    if (!target) {
      missingTargetIds.push(targetId);
      continue;
    }
    if (targetIsSelectableForRun(target)) {
      selectableTargetIds.push(targetId);
    } else {
      unavailableTargetIds.push(targetId);
    }
  }
  return {
    selectableTargetIds,
    unavailableTargetIds,
    missingTargetIds,
  };
}

function errorRecoveryRerunTitle(row: ErrorRecoveryRow, availability: ErrorRecoveryTargetAvailability) {
  if (row.packIds.length !== 1) {
    return 'Filter to one benchmark pack before opening a scoped rerun';
  }
  if (!availability.selectableTargetIds.length) {
    return 'Fix, enable, validate, or recreate the affected target before opening a scoped rerun';
  }
  const skippedTargets = [...availability.unavailableTargetIds, ...availability.missingTargetIds];
  if (skippedTargets.length) {
    return `Open Run Builder for selectable targets; skips unavailable target(s): ${previewList(skippedTargets)}`;
  }
  return 'Open Run Builder for this failed target/task scope';
}

function localRuntimeCanAddTarget(runtime: LocalRuntime, model: string) {
  return runtime.status !== 'error' && Boolean(model.trim());
}

function localRuntimeToolSupported(runtimeId: string) {
  return ['ollama', 'llama-cpp', 'vllm', 'mlx-lm'].includes(runtimeId);
}

function targetCompatibleWithPack(target: Target, pack?: BenchmarkPack) {
  if (!pack || !pack.supportedTargetKinds.length) {
    return true;
  }
  return pack.supportedTargetKinds.includes(target.kind);
}

function recommendedRunPackIdForTarget(target: Target) {
  if (target.kind === 'direct_model' || target.kind === 'harnessed_model') {
    return connectivityBenchmarkPackId;
  }
  if (target.kind === 'benchmark_harness') {
    return 'security-defensive';
  }
  return 'quick-smoke';
}

function modelBenchmarkPackOptions(packs: BenchmarkPack[]): BenchmarkPackOption[] {
  const options = packs
    .filter(pack => pack.id.startsWith('llm-')
      && pack.taskTypes.includes('prompt')
      && pack.supportedTargetKinds.includes('direct_model'))
    .sort((left, right) => modelBenchmarkPackSortIndex(left.id) - modelBenchmarkPackSortIndex(right.id)
      || left.name.localeCompare(right.name))
    .map(pack => ({ id: pack.id, label: pack.name }));
  return options.length ? options : fallbackModelBenchmarkPacks;
}

function modelBenchmarkPackSortIndex(packId: string) {
  const index = modelBenchmarkPackOrder.indexOf(packId);
  return index === -1 ? modelBenchmarkPackOrder.length : index;
}

function benchmarkPackLabel(packId: string, options: BenchmarkPackOption[] = fallbackModelBenchmarkPacks) {
  return options.find(pack => pack.id === packId)?.label ?? packId;
}

function resolveModelBenchmarkPackId(requestedPackId: string | undefined, options: BenchmarkPackOption[], packs: BenchmarkPack[]) {
  if (requestedPackId && options.some(pack => pack.id === requestedPackId)) {
    return requestedPackId;
  }
  const recommendedPackId = recommendedComparisonPackId(packs);
  if (options.some(pack => pack.id === recommendedPackId)) {
    return recommendedPackId;
  }
  return options[0]?.id ?? defaultModelComparisonPackId;
}

function automaticModelBenchmarkSettings(packId: string) {
  if (packId === connectivityBenchmarkPackId) {
    return { repetitions: 1, warmupRuns: 0, concurrency: 1 };
  }
  return { repetitions: 3, warmupRuns: 1, concurrency: 1 };
}

function packUsesModelSelectionDefaults(pack: BenchmarkPack) {
  return pack.taskTypes.includes('prompt')
    && pack.id !== connectivityBenchmarkPackId
    && pack.evidenceProfile !== 'connectivity_smoke';
}

function automaticModelBenchmarkMaxCostUsd(packId: string) {
  return packId === connectivityBenchmarkPackId ? automaticConnectivityMaxCostUsd : defaultComparisonMaxCostUsd;
}

function automaticModelRunBuilderIntent(target: Target, benchmarkPackId: string): RunBuilderIntent {
  const settings = automaticModelBenchmarkSettings(benchmarkPackId);
  return {
    targetIds: [target.id],
    benchmarkPackId,
    repetitions: settings.repetitions,
    warmupRuns: settings.warmupRuns,
    concurrency: settings.concurrency,
    maxCostUsd: automaticModelBenchmarkMaxCostUsd(benchmarkPackId),
  };
}

function scopedModelBenchmarkTargetIdsForTarget(target: Target, targets: Target[], scopedTargetIds: string[], options: { requirePricedCloud?: boolean } = {}) {
  if (!scopedTargetIds.length || !targetIsSelectableModel(target)) {
    return null;
  }
  const targetIsLocal = isLocalModelTarget(target);
  const targetIsCloud = isCloudModelTarget(target);
  if (!targetIsLocal && !targetIsCloud) {
    return null;
  }
  if (options.requirePricedCloud && targetIsCloud && !targetHasInputOutputPricing(target)) {
    return null;
  }
  const targetById = new Map(targetListWithOverride(target, targets).map(candidate => [candidate.id, candidate]));
  const counterparts = uniqueIdsInOrder(scopedTargetIds)
    .filter(id => id !== target.id)
    .map(id => targetById.get(id))
    .filter((candidate): candidate is Target => Boolean(candidate))
    .filter(candidate => targetIsSelectableModel(candidate))
    .filter(candidate => targetIsLocal ? isCloudModelTarget(candidate) : isLocalModelTarget(candidate))
    .filter(candidate => !options.requirePricedCloud || !isCloudModelTarget(candidate) || targetHasInputOutputPricing(candidate));
  if (!counterparts.length) {
    return null;
  }
  const counterpartIds = counterparts.map(candidate => candidate.id);
  return targetIsLocal ? [target.id, ...counterpartIds] : [...counterpartIds, target.id];
}

function automaticModelBenchmarkIntentForTarget(target: Target, targets: Target[], benchmarkPackId: string, scopedTargetIds: string[] = []): RunBuilderIntent {
  const settings = automaticModelBenchmarkSettings(benchmarkPackId);
  const scopedRunTargetIds = scopedModelBenchmarkTargetIdsForTarget(target, targets, scopedTargetIds);
  if (scopedRunTargetIds) {
    return {
      ...localCloudRunBuilderIntent(scopedRunTargetIds, benchmarkPackId),
      repetitions: settings.repetitions,
      warmupRuns: settings.warmupRuns,
      concurrency: Math.min(2, Math.max(1, scopedRunTargetIds.length)),
      maxCostUsd: automaticModelBenchmarkMaxCostUsd(benchmarkPackId),
    };
  }
  const comparisonIntent = modelComparisonIntentForTarget(target, targets, benchmarkPackId);
  if (comparisonIntent) {
    return {
      ...comparisonIntent,
      repetitions: settings.repetitions,
      warmupRuns: settings.warmupRuns,
      concurrency: Math.min(2, Math.max(1, comparisonIntent.targetIds.length)),
      maxCostUsd: automaticModelBenchmarkMaxCostUsd(benchmarkPackId),
    };
  }
  return automaticModelRunBuilderIntent(target, benchmarkPackId);
}

function plannedModelTarget(
  id: string,
  name: string,
  adapterId: string,
  baseUrl: string,
  pricing: Partial<Pick<Target, 'inputPriceUsdPerMillionTokens' | 'outputPriceUsdPerMillionTokens' | 'cacheReadPriceUsdPerMillionTokens' | 'cacheWritePriceUsdPerMillionTokens'>> = {},
): Target {
  const classification = plannedModelTargetClassification(adapterId, baseUrl);
  return {
    id,
    name,
    kind: 'direct_model',
    adapterId,
    status: 'unknown',
    enabled: true,
    isLocalModel: classification.local,
    isCloudModel: classification.cloud,
    ...pricing,
  };
}

function plannedModelTargetClassification(adapterId: string, baseUrl: string) {
  if (cloudModelAdapters.has(adapterId)) {
    return { local: false, cloud: true };
  }
  if (localModelAdapters.has(adapterId)) {
    return { local: true, cloud: false };
  }
  if (baseUrl && dashboardBaseUrlLooksRemote(baseUrl)) {
    return { local: false, cloud: true };
  }
  return { local: true, cloud: false };
}

function runBuilderIntentForTarget(target: Target, benchmarkPackId = recommendedRunPackIdForTarget(target)): RunBuilderIntent {
  const intent: RunBuilderIntent = {
    targetIds: [target.id],
    benchmarkPackId,
  };
  if (target.kind === 'direct_model' || target.kind === 'harnessed_model') {
    return {
      ...intent,
      repetitions: 3,
      warmupRuns: 1,
      concurrency: 1,
    };
  }
  return intent;
}

function modelComparisonIntentForTarget(target: Target, targets: Target[], benchmarkPackId: string, options: { requirePricedCloud?: boolean } = {}): RunBuilderIntent | null {
  if (!targetIsSelectableModel(target)) {
    return null;
  }
  const targetIsLocal = isLocalModelTarget(target);
  const targetIsCloud = isCloudModelTarget(target);
  if (!targetIsLocal && !targetIsCloud) {
    return null;
  }
  if (options.requirePricedCloud && targetIsCloud && !targetHasInputOutputPricing(target)) {
    return null;
  }
  const counterparts = targets
    .filter(candidate => candidate.id !== target.id
      && targetIsSelectableModel(candidate)
      && (targetIsLocal ? isCloudModelTarget(candidate) : isLocalModelTarget(candidate))
      && (!options.requirePricedCloud || !isCloudModelTarget(candidate) || targetHasInputOutputPricing(candidate)))
    .sort(compareModelCounterpartPriority);
  const counterpart = counterparts[0];
  if (!counterpart) {
    return null;
  }
  const orderedIds = targetIsLocal ? [target.id, counterpart.id] : [counterpart.id, target.id];
  return localCloudRunBuilderIntent(orderedIds, benchmarkPackId);
}

function compareModelCounterpartPriority(left: Target, right: Target) {
  return Number(targetHasInputOutputPricing(right)) - Number(targetHasInputOutputPricing(left))
    || left.id.localeCompare(right.id);
}

function localCloudRunBuilderIntent(targetIds: string[], benchmarkPackId = defaultModelComparisonPackId): RunBuilderIntent {
  return {
    targetIds,
    benchmarkPackId,
    repetitions: 3,
    warmupRuns: 1,
    concurrency: Math.min(2, Math.max(1, targetIds.length)),
    maxCostUsd: defaultComparisonMaxCostUsd,
  };
}

function recommendedHarnessPackForCommand(command: string) {
  const normalized = command.toLowerCase();
  if (normalized.includes('evalplus')) {
    return 'evalplus';
  }
  if (normalized.includes('terminal-bench') || normalized.includes('terminal_bench')) {
    return 'terminal-bench-subset';
  }
  if (normalized.includes('swebench') || normalized.includes('swe-bench')) {
    return 'swebench-lite-subset';
  }
  if (normalized.includes('aider')) {
    return 'aider-polyglot-subset';
  }
  return 'security-defensive';
}

function harnessPresetIdFromConfig(presetId: string, command: string) {
  if (harnessPresets.some(preset => preset.id === presetId)) {
    return presetId;
  }
  const recommendedPack = recommendedHarnessPackForCommand(command);
  const preset = harnessPresets.find(item => item.benchmarkPackId === recommendedPack);
  return preset?.id ?? 'custom';
}

function envPassthroughFromText(value: string) {
  return Array.from(new Set(value.split(/[\s,]+/).map(item => item.trim()).filter(Boolean)));
}

function isValidEnvName(value: string) {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value);
}

function formValueFromEnvPassthrough(value: unknown) {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === 'string' && Boolean(item.trim())).join(', ');
  }
  return formValueFromUnknown(value);
}

function preferredRunPackForTargets(targetIds: string[], targets: Target[], packs: BenchmarkPack[]) {
  const selectedTargets = targets.filter(target => targetIds.includes(target.id));
  const selectedModelTargets = selectedTargets.filter(targetIsSelectableModel);
  const hasLocalModel = selectedModelTargets.some(isLocalModelTarget);
  const hasCloudModel = selectedModelTargets.some(isCloudModelTarget);
  if (hasLocalModel && hasCloudModel) {
    return recommendedComparisonPackId(packs);
  }
  const firstTarget = selectedTargets[0];
  const preferred = firstTarget ? recommendedRunPackIdForTarget(firstTarget) : 'quick-smoke';
  if (packs.some(pack => pack.id === preferred)) {
    return preferred;
  }
  if (packs.some(pack => pack.id === defaultModelComparisonPackId)) {
    return defaultModelComparisonPackId;
  }
  return packs[0]?.id ?? '';
}

function recommendedComparisonPackId(packs: BenchmarkPack[]) {
  const modelPromptPacks = packs.filter(pack => pack.id.startsWith('llm-')
    && pack.id !== connectivityBenchmarkPackId
    && pack.taskTypes.includes('prompt')
    && pack.supportedTargetKinds.includes('direct_model'));
  const availableIds = new Set(modelPromptPacks.map(pack => pack.id));
  for (const packId of modelSelectionPackPreference) {
    if (availableIds.has(packId)) {
      return packId;
    }
  }
  const promptComparisonPack = modelPromptPacks.find(pack => pack.evidenceProfile === 'prompt_comparison');
  if (promptComparisonPack) {
    return promptComparisonPack.id;
  }
  if (modelPromptPacks[0]) {
    return modelPromptPacks[0].id;
  }
  if (packs.some(pack => pack.id === connectivityBenchmarkPackId)) {
    return connectivityBenchmarkPackId;
  }
  return defaultModelComparisonPackId;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function resultMatchesDateWindow(result: RunResult, window: DateWindow) {
  if (window === 'all') {
    return true;
  }
  const started = parseResultStartedAt(result.started_at);
  if (started == null) {
    return false;
  }
  if (window === 'today') {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    return started >= today.getTime();
  }
  const days = window === '7d' ? 7 : 30;
  return started >= Date.now() - days * 24 * 60 * 60 * 1000;
}

function parseResultStartedAt(value: string | null | undefined) {
  if (!value) {
    return null;
  }
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function dateWindowLabel(window: DateWindow) {
  return dateWindowOptions.find(option => option.id === window)?.label ?? 'All time';
}

function exportFormatLabel(format: 'jsonl' | 'csv' | 'markdown' | 'analysis') {
  const labels = {
    jsonl: 'JSONL',
    csv: 'CSV',
    markdown: 'Markdown report',
    analysis: 'Analysis JSON',
  };
  return labels[format];
}

function formatDateTime(value: string | null | undefined) {
  const parsed = parseResultStartedAt(value);
  if (parsed == null) {
    return value || '-';
  }
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(parsed));
}

function chartRowLabel(row: ComparisonRow) {
  return `${row.targetId} · ${row.packId}`;
}

function taskRowLabel(row: TaskComparisonRow) {
  return `${row.taskId} · ${row.targetId}`;
}

function totalTokens(result: RunResult) {
  if (typeof result.total_tokens === 'number') {
    return result.total_tokens;
  }
  if (typeof result.prompt_tokens === 'number' || typeof result.completion_tokens === 'number') {
    return (result.prompt_tokens ?? 0) + (result.completion_tokens ?? 0);
  }
  return null;
}

function formatPercent(value: number | null) {
  if (value == null) {
    return '-';
  }
  return `${Math.round(value * 100)}%`;
}

function formatPercentPointDelta(value: number) {
  return `${value >= 0 ? '+' : ''}${Math.round(value * 100)} pp`;
}

function formatPercentRange(low: number | null, high: number | null) {
  if (low == null || high == null) {
    return '-';
  }
  return `${formatPercent(low)}-${formatPercent(high)}`;
}

function formatNumber(value: number | null) {
  return value == null ? '-' : value.toFixed(2);
}

function formatNumberDelta(value: number | null) {
  return value == null ? '-' : `${value >= 0 ? '+' : ''}${value.toFixed(2)}`;
}

function formatHttpStatus(value: number | null | undefined) {
  if (value == null || !Number.isFinite(value)) {
    return '-';
  }
  return String(Math.round(value));
}

function formatHttpStatusCounts(counts: Map<number, number>) {
  if (!counts.size) {
    return '-';
  }
  return Array.from(counts.entries())
    .sort(([left], [right]) => left - right)
    .map(([status, count]) => count === 1 ? String(status) : `${status} (${count})`)
    .join(', ');
}

function formatTextCounts(counts: Map<string, number>) {
  if (!counts.size) {
    return '-';
  }
  return Array.from(counts.entries())
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([value, count]) => count === 1 ? value : `${value} (${count})`)
    .join(', ');
}

function formatTextCountsWithSingles(counts: Map<string, number>) {
  if (!counts.size) {
    return '-';
  }
  return Array.from(counts.entries())
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([value, count]) => `${value} (${count})`)
    .join(', ');
}

function sumMapValues(counts: Map<unknown, number>) {
  return Array.from(counts.values()).reduce((sum, count) => sum + count, 0);
}

function formatInteger(value: number | null) {
  return value == null ? '-' : Math.round(value).toLocaleString();
}

function formatNumberWithSpread(value: number | null, spread: number | null) {
  if (value == null) {
    return '-';
  }
  return `${value.toFixed(2)} / ${spread == null ? '-' : spread.toFixed(2)}`;
}

function formatNumberDistribution(medianValue: number | null, min: number | null, max: number | null) {
  if (medianValue == null) {
    return '-';
  }
  return `${formatNumber(medianValue)} / ${formatNumber(min)} / ${formatNumber(max)}`;
}

function formatMs(value: number | null) {
  return value == null ? '-' : `${Math.round(value)} ms`;
}

function formatMsDistribution(medianValue: number | null, min: number | null, max: number | null) {
  if (medianValue == null) {
    return '-';
  }
  return `${formatMs(medianValue)} / ${formatMs(min)} / ${formatMs(max)}`;
}

function formatMsDelta(value: number | null) {
  return value == null ? '-' : `${value >= 0 ? '+' : ''}${Math.round(value)} ms`;
}

function formatMsPair(first: number | null, second: number | null) {
  if (first == null) {
    return '-';
  }
  return `${Math.round(first)} / ${second == null ? '-' : Math.round(second)} ms`;
}

function formatDurationSeconds(value: number | null | undefined) {
  if (value == null || !Number.isFinite(value)) {
    return '-';
  }
  const seconds = Math.max(0, Math.round(value));
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return remainingMinutes ? `${hours}h ${remainingMinutes}m` : `${hours}h`;
}

function percentile(values: number[], rank: number) {
  if (!values.length) {
    return null;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(rank * sorted.length) - 1));
  return sorted[index];
}

function median(values: number[]) {
  if (!values.length) {
    return null;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const midpoint = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[midpoint - 1] + sorted[midpoint]) / 2
    : sorted[midpoint];
}

function minValue(values: number[]) {
  return values.length ? Math.min(...values) : null;
}

function maxValue(values: number[]) {
  return values.length ? Math.max(...values) : null;
}

function average(values: number[]) {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null;
}

function stdDev(values: number[]) {
  if (values.length < 2) {
    return null;
  }
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  const variance = values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / (values.length - 1);
  return Math.sqrt(variance);
}

function formatCost(value: number | null) {
  if (value == null) {
    return '-';
  }
  if (value === 0) {
    return '$0';
  }
  return value < 0.01 ? `$${value.toFixed(6)}` : `$${value.toFixed(4)}`;
}

function formatCostDelta(value: number | null) {
  if (value == null) {
    return '-';
  }
  if (value === 0) {
    return '$0';
  }
  const sign = value >= 0 ? '+' : '-';
  const absolute = Math.abs(value);
  return absolute < 0.01 ? `${sign}$${absolute.toFixed(6)}` : `${sign}$${absolute.toFixed(4)}`;
}

function formatPricePair(input?: number | null, output?: number | null, cacheRead?: number | null, cacheWrite?: number | null) {
  if (input == null && output == null && cacheRead == null && cacheWrite == null) {
    return 'manual price';
  }
  const base = `${input == null ? '-' : `$${input}`} / ${output == null ? '-' : `$${output}`} per 1M`;
  const cacheParts = [
    cacheRead == null ? null : `cache read $${cacheRead}`,
    cacheWrite == null ? null : `cache write $${cacheWrite}`,
  ].filter(Boolean);
  return cacheParts.length ? `${base}; ${cacheParts.join(', ')}` : base;
}

function parseOptionalNonNegativeNumber(value: string, label: string): { value?: number; error?: string } {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return { error: `${label} must be a non-negative number` };
  }
  return { value: parsed };
}

function parsePositiveIntegerInRange(value: string, label: string, min: number, max: number): { value?: number; error?: string } {
  const trimmed = value.trim();
  if (!trimmed) {
    return { error: `${label} is required` };
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed < min || parsed > max) {
    return { error: `${label} must be an integer between ${min} and ${max}` };
  }
  return { value: parsed };
}

function parseOptionalNumberInRange(value: string, label: string, min: number, max: number): { value?: number; error?: string } {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed < min || parsed > max) {
    return { error: `${label} must be between ${min} and ${max}` };
  }
  return { value: parsed };
}

function parseOptionalPositiveInteger(value: string, label: string): { value?: number; error?: string } {
  const parsed = parseOptionalInteger(value, label);
  if (parsed.error || parsed.value == null) {
    return parsed;
  }
  if (parsed.value < 1) {
    return { error: `${label} must be at least 1` };
  }
  return parsed;
}

function parseOptionalInteger(value: string, label: string): { value?: number; error?: string } {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed)) {
    return { error: `${label} must be an integer` };
  }
  return { value: parsed };
}

function parseOptionalIntegerInRange(value: string, label: string, min: number, max: number): { value?: number; error?: string } {
  const parsed = parseOptionalInteger(value, label);
  if (parsed.error || parsed.value == null) {
    return parsed;
  }
  if (parsed.value < min || parsed.value > max) {
    return { error: `${label} must be an integer between ${min} and ${max}` };
  }
  return parsed;
}

function SettingsPage({ busy, targets, adapters, packs, setBusy, setMessage, refresh, openRunBuilder, openResultsForGroup, openTargetRepair, openTargetSetup, setupIntent, onSetupIntentConsumed }: { busy: boolean; targets: Target[]; adapters: Adapter[]; packs: BenchmarkPack[]; setBusy: (busy: boolean) => void; setMessage: (message: string) => void; refresh: () => Promise<void>; openRunBuilder: (intent: RunBuilderIntent) => void; openResultsForGroup: (groupId: string, runId?: string) => void; openTargetRepair: (intent: Omit<TargetRepairIntent, 'nonce'>) => void; openTargetSetup: (intent: Omit<TargetSetupIntent, 'nonce'>) => void; setupIntent: HuggingFaceLocalSetupIntent | null; onSetupIntentConsumed: () => void }) {
  const [hf, setHf] = useState<HuggingFaceStatus | null>(null);
  const [token, setToken] = useState('');
  const [repoId, setRepoId] = useState('');
  const [filename, setFilename] = useState('');
  const [revision, setRevision] = useState('');
  const [port, setPort] = useState(8080);
  const [context, setContext] = useState(2048);
  const [installLog, setInstallLog] = useState('');
  const [modelQuery, setModelQuery] = useState('');
  const [modelSort, setModelSort] = useState('trendingScore');
  const [modelResults, setModelResults] = useState<HuggingFaceModel[]>([]);
  const [browserBusy, setBrowserBusy] = useState(false);
  const [modelFileDetails, setModelFileDetails] = useState<GgufFileDetail[]>([]);
  const [modelFileBusy, setModelFileBusy] = useState(false);
  const [downloadLog, setDownloadLog] = useState('');
  const [downloadPlan, setDownloadPlan] = useState<HuggingFaceDownloadPlan | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<HuggingFaceDownloadProgress | null>(null);
  const [downloadFailed, setDownloadFailed] = useState(false);
  const [downloadJobs, setDownloadJobs] = useState<HuggingFaceDownloadJob[]>([]);
  const [activeDownloadJobId, setActiveDownloadJobId] = useState('');
  const [serverJobs, setServerJobs] = useState<HuggingFaceServerJob[]>([]);
  const [activeServerJobId, setActiveServerJobId] = useState('');
  const [localAdvancedOpen, setLocalAdvancedOpen] = useState(false);
  const [autoStartAfterDownload, setAutoStartAfterDownload] = useState(true);
  const [autoBenchmarkAfterStart, setAutoBenchmarkAfterStart] = useState(true);
  const [autoCompareAfterStart, setAutoCompareAfterStart] = useState(true);
  const [autoCompareCloudTargetId, setAutoCompareCloudTargetId] = useState('');
  const [autoBenchmarkPackId, setAutoBenchmarkPackId] = useState('llm-basics');
  const [preflight, setPreflight] = useState<ModelPreflight | null>(null);
  const [preflightBusy, setPreflightBusy] = useState(false);
  const modelBenchmarkPacks = useMemo(() => modelBenchmarkPackOptions(packs), [packs]);
  const autoStartDownloadJobIdsRef = useRef<Set<string>>(new Set());
  const autoRegisterServerJobIdsRef = useRef<Set<string>>(new Set());
  const recoveredDownloadJobIdsRef = useRef<Set<string>>(new Set());
  const recoveredServerJobIdsRef = useRef<Set<string>>(new Set());
  const activeDownloadJob = downloadJobs.find(job => job.id === activeDownloadJobId);
  const activeDownloadInProgress = Boolean(activeDownloadJob && isDownloadJobActive(activeDownloadJob));
  const activeServerJob = serverJobs.find(job => job.id === activeServerJobId);
  const activeServerStartInProgress = Boolean(activeServerJob && isServerJobActive(activeServerJob));
  const activeDownloadCount = downloadJobs.filter(isDownloadJobActive).length;
  const activeServerStartCount = serverJobs.filter(isServerJobActive).length;
  const downloadedGgufCount = hf?.models.reduce((total, model) => total + model.ggufFiles.length, 0) ?? 0;
  const settingsComparisonTargets = dashboardLocalCloudComparisonTargets(targets);
  const selectableCloudTargets = useMemo(() => targets.filter(target => targetIsSelectableModel(target) && isCloudModelTarget(target)), [targets]);
  const selectablePricedCloudTargets = useMemo(() => selectableCloudTargets
    .filter(targetHasInputOutputPricing)
    .sort(compareDashboardComparisonTargetPriority), [selectableCloudTargets]);
  const selectedAutoCompareCloudTarget = selectablePricedCloudTargets.find(target => target.id === autoCompareCloudTargetId)
    ?? selectablePricedCloudTargets[0]
    ?? null;
  const selectableCloudTargetCount = selectableCloudTargets.length;
  const selectablePricedCloudTargetCount = selectablePricedCloudTargets.length;
  const unpricedCloudComparisonTargetIds = selectableCloudTargets
    .filter(target => !targetHasInputOutputPricing(target))
    .map(target => target.id);
  const cloudSetupAdapterId = usePreferredCloudSetupAdapterId(adapters);
  const cloudComparisonNeedsPricing = Boolean(selectableCloudTargetCount && unpricedCloudComparisonTargetIds.length && !selectablePricedCloudTargetCount);
  const autoCompareReady = autoBenchmarkAfterStart && autoCompareAfterStart && Boolean(selectedAutoCompareCloudTarget);
  const cloudComparisonStatus = hfCloudComparisonStatus(selectableCloudTargetCount, selectablePricedCloudTargetCount);
  const cloudComparisonActionLabel = cloudComparisonNeedsPricing ? 'Add pricing' : 'Add cloud target';
  const cloudComparisonActionTitle = cloudComparisonNeedsPricing
    ? 'Add input/output pricing to an existing cloud target'
    : 'Create and validate a cloud target for automatic comparison';
  const autoCompareCloudTargetLabel = selectedAutoCompareCloudTarget ? hfCloudTargetLabel(selectedAutoCompareCloudTarget) : '';
  const hfHandoffSteps = hfAutomaticHandoffSteps({
    repoId: repoId.trim(),
    selectedFile: filename || downloadPlan?.selectedFile || activeDownloadJob?.selectedFile || '',
    hf,
    autoStartAfterDownload,
    autoBenchmarkAfterStart,
    autoCompareAfterStart,
    autoBenchmarkPackId,
    modelBenchmarkPacks,
    selectableCloudTargetCount,
    selectablePricedCloudTargetCount,
    selectedCloudTargetLabel: autoCompareCloudTargetLabel,
    port,
    context,
  });
  const hfHandoffReady = hfHandoffSteps.every(step => step.tone === 'ok' || step.optional);

  function openCloudComparisonSetup() {
    if (cloudComparisonNeedsPricing) {
      openTargetRepair({ targetIds: unpricedCloudComparisonTargetIds, code: 'pricing_assumption' });
      setMessage(`Add input/output pricing before automatic local/cloud comparison: ${previewList(unpricedCloudComparisonTargetIds)}`);
      return;
    }
    openTargetSetup({ adapterId: cloudSetupAdapterId, code: 'missing_key', benchmarkPackId: autoBenchmarkPackId, targetIds: settingsComparisonTargets.setupLocalTargetIds });
  }

  function useLocalOnlyHandoff() {
    setAutoCompareAfterStart(false);
    setMessage(`Automatic handoff will run ${benchmarkPackLabel(autoBenchmarkPackId, modelBenchmarkPacks)} locally only. Add a priced cloud target later to compare the same pack.`);
  }

  useEffect(() => {
    if (!setupIntent) {
      return;
    }
    const packId = resolveModelBenchmarkPackId(setupIntent.benchmarkPackId, modelBenchmarkPacks, packs);
    const handoffTargetIds = setupIntent.targetIds?.filter(id => targets.some(target => target.id === id)) ?? [];
    const handoffCloudTargets = handoffTargetIds
      .map(id => targets.find(target => target.id === id))
      .filter((target): target is Target => Boolean(target && isCloudModelTarget(target)));
    const handoffPricedCloudTarget = handoffCloudTargets
      .map(target => selectablePricedCloudTargets.find(candidate => candidate.id === target.id))
      .find((target): target is Target => Boolean(target));
    setAutoStartAfterDownload(true);
    setAutoBenchmarkAfterStart(true);
    setAutoCompareAfterStart(handoffCloudTargets.length ? Boolean(handoffPricedCloudTarget) : true);
    setAutoBenchmarkPackId(packId);
    if (handoffPricedCloudTarget) {
      setAutoCompareCloudTargetId(handoffPricedCloudTarget.id);
    }
    setMessage(huggingFaceLocalModelSetupMessage(targets, handoffTargetIds, packId));
    onSetupIntentConsumed();
  }, [setupIntent, modelBenchmarkPacks, packs, targets, selectablePricedCloudTargets, setMessage, onSetupIntentConsumed]);

  useEffect(() => {
    if (!selectablePricedCloudTargets.length) {
      if (autoCompareCloudTargetId) {
        setAutoCompareCloudTargetId('');
      }
      return;
    }
    if (!selectablePricedCloudTargets.some(target => target.id === autoCompareCloudTargetId)) {
      setAutoCompareCloudTargetId(selectablePricedCloudTargets[0].id);
    }
  }, [selectablePricedCloudTargets, autoCompareCloudTargetId]);

  async function refreshHf() {
    setHf(await huggingFaceStatus());
  }

  useEffect(() => {
    refreshHf().catch(error => setMessage(String(error)));
    browseModels().catch(error => setMessage(String(error)));
    Promise.all([listHuggingFaceDownloadJobs(), listHuggingFaceServerJobs()])
      .then(([nextDownloadJobs, nextServerJobs]) => {
        setDownloadJobs(nextDownloadJobs);
        setServerJobs(nextServerJobs);
        const activeDownload = nextDownloadJobs.find(isDownloadJobActive);
        const activeServer = nextServerJobs.find(isServerJobActive);
        if (activeDownload) {
          setActiveDownloadJobId(activeDownload.id);
          setDownloadProgress(progressFromDownloadJob(activeDownload));
        }
        if (activeServer) {
          setActiveServerJobId(activeServer.id);
        }
        if (!activeDownload && !activeServer) {
          recoverFinishedHfHandoff(nextDownloadJobs, nextServerJobs).catch(error => setMessage(String(error)));
        }
      })
      .catch(error => setMessage(String(error)));
  }, []);

  async function recoverFinishedHfHandoff(
    nextDownloadJobs = downloadJobs,
    nextServerJobs = serverJobs,
  ) {
    const downloadJob = newestJob(nextDownloadJobs.filter(job => completedDownloadNeedsStart(job, nextServerJobs)));
    if (downloadJob) {
      recoveredDownloadJobIdsRef.current.add(downloadJob.id);
      setMessage(`Recovering completed download handoff for ${downloadJob.repoId}`);
      await handleFinishedDownloadJob(downloadJob);
      return;
    }

    const existingTargets = await listTargets();
    const serverJob = newestJob(nextServerJobs.filter(job => completedServerNeedsTarget(job, existingTargets)));
    if (serverJob) {
      recoveredServerJobIdsRef.current.add(serverJob.id);
      setMessage(`Recovering completed server handoff for ${serverJob.repoId}`);
      await handleFinishedServerJob(serverJob);
    }
  }

  function completedDownloadNeedsStart(job: HuggingFaceDownloadJob, existingServerJobs: HuggingFaceServerJob[]) {
    return job.status === 'completed'
      && Boolean(job.startAfterDownload)
      && Boolean(job.model)
      && !recoveredDownloadJobIdsRef.current.has(job.id)
      && !existingServerJobs.some(serverJob => serverJobMatchesDownload(serverJob, job));
  }

  function completedServerNeedsTarget(job: HuggingFaceServerJob, existingTargets: Target[]) {
    return job.status === 'completed'
      && Boolean(job.registerTargetAfterStart)
      && !recoveredServerJobIdsRef.current.has(job.id)
      && !existingTargets.some(target => target.id === hfLocalTargetId(job.repoId, job.selectedFile ?? '', job.port));
  }

  useEffect(() => {
    if (!activeDownloadJobId) {
      return undefined;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    async function poll() {
      try {
        const job = await getHuggingFaceDownloadJob(activeDownloadJobId);
        if (cancelled) {
          return;
        }
        if (job) {
          setDownloadJobs(current => mergeDownloadJob(current, job));
          setDownloadProgress(progressFromDownloadJob(job));
          setDownloadFailed(job.status === 'failed');
          if (!isDownloadJobActive(job)) {
            await handleFinishedDownloadJob(job);
            return;
          }
        }
      } catch (error) {
        if (!cancelled) {
          setMessage(String(error));
        }
      }
      timer = setTimeout(poll, 1000);
    }

    poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [activeDownloadJobId]);

  async function handleFinishedDownloadJob(job: HuggingFaceDownloadJob) {
    setActiveDownloadJobId('');
    setDownloadJobs(current => mergeDownloadJob(current, job));
    setDownloadProgress(progressFromDownloadJob(job));
    setDownloadFailed(job.status === 'failed');
    const shouldAutoStart = autoStartDownloadJobIdsRef.current.delete(job.id) || Boolean(job.startAfterDownload);
    const nextStatus = await huggingFaceStatus();
    setHf(nextStatus);
    if (job.status === 'completed' && job.model) {
      const selectedFile = job.model.selectedFile ?? job.selectedFile ?? '';
      setRepoId(job.repoId);
      setFilename(selectedFile);
      setModelFileDetails(job.model.ggufFileDetails);
      setDownloadLog(job.model.downloadLog ?? job.message);
      if (shouldAutoStart) {
        const benchmarkNote = hfBenchmarkHandoffNote(
          job.runConnectivityAfterStart ?? autoBenchmarkAfterStart,
          job.autoBenchmarkPackId ?? autoBenchmarkPackId,
          modelBenchmarkPacks,
          Boolean(job.autoCompareAfterStart) && (job.autoBenchmarkTargetIds?.length ?? 0) > 1,
        );
        setMessage(`Downloaded ${job.repoId}; starting local server${benchmarkNote}`);
        try {
          const serverJobId = await startModelFromSelection(
            nextStatus,
            serverSettingsFromDownloadJob(job),
            selectedFile,
            job.repoId,
            job.runConnectivityAfterStart ?? autoBenchmarkAfterStart,
            job.autoBenchmarkPackId ?? autoBenchmarkPackId,
            Boolean(job.autoCompareAfterStart),
          );
          if (serverJobId) {
            setMessage(`Downloaded ${job.repoId}; server start job ${serverJobId.slice(0, 8)} queued${benchmarkNote}`);
          }
        } catch (startError) {
          setDownloadLog([job.model.downloadLog ?? '', `Start failed: ${String(startError)}`].filter(Boolean).join('\n'));
          setMessage(`Downloaded ${job.repoId}, but start failed: ${String(startError)}`);
        }
      } else {
        setMessage(job.message);
      }
      return;
    }
    if (job.status === 'failed') {
      setDownloadLog(job.error ?? job.message);
    }
    setMessage(job.message);
  }

  useEffect(() => {
    if (!activeServerJobId) {
      return undefined;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    async function poll() {
      try {
        const job = await getHuggingFaceServerJob(activeServerJobId);
        if (cancelled) {
          return;
        }
        if (job) {
          setServerJobs(current => mergeServerJob(current, job));
          if (!isServerJobActive(job)) {
            await handleFinishedServerJob(job);
            return;
          }
        }
      } catch (error) {
        if (!cancelled) {
          setMessage(String(error));
        }
      }
      timer = setTimeout(poll, 1000);
    }

    poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [activeServerJobId]);

  async function handleFinishedServerJob(job: HuggingFaceServerJob) {
    setActiveServerJobId('');
    const nextStatus = job.serverStatus ?? await huggingFaceStatus();
    setHf(nextStatus);
    const shouldRegister = autoRegisterServerJobIdsRef.current.delete(job.id) || Boolean(job.registerTargetAfterStart);
    if (job.status === 'completed' && shouldRegister) {
      try {
        const target = await registerLocalTarget(nextStatus, { port: job.port, context: job.context }, job.selectedFile ?? '', job.repoId);
        const validation = await validateTarget(target.id);
        await finishLocalTargetHandoff(
          target,
          validation,
          'Server ready and target saved',
          job.runConnectivityAfterStart ?? autoBenchmarkAfterStart,
          job.autoBenchmarkPackId ?? autoBenchmarkPackId,
          Boolean(job.autoCompareAfterStart),
        );
      } catch (error) {
        setMessage(`Server ready, but target registration failed: ${String(error)}`);
      }
      return;
    }
    autoRegisterServerJobIdsRef.current.delete(job.id);
    setMessage(job.message);
  }

  async function saveToken() {
    setBusy(true);
    try {
      await savePendingHfToken();
      setMessage('Hugging Face token saved to Keychain');
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function savePendingHfToken() {
    const pendingToken = token.trim();
    if (!pendingToken) {
      return hf;
    }
    await saveHuggingFaceToken(pendingToken);
    setToken('');
    const nextStatus = await huggingFaceStatus();
    setHf(nextStatus);
    return nextStatus;
  }

  async function installMissingTools() {
    setBusy(true);
    try {
      const installHf = !hf?.hfCliAvailable;
      const installPython = installHf && !hf?.pythonSupported;
      const result = await installHuggingFaceTools(installHf, !hf?.llamaServerAvailable, installPython);
      setInstallLog(result.log);
      await refreshHf();
      await refresh();
      setMessage(`Hugging Face setup ${result.status}`);
    } catch (error) {
      setInstallLog(String(error));
      setMessage('Hugging Face setup failed');
    } finally {
      setBusy(false);
    }
  }

  async function copyHfCachePath() {
    if (!hf?.cacheDir) {
      return;
    }
    await navigator.clipboard.writeText(hf.cacheDir);
    setMessage('Copied Hugging Face model cache path');
  }

  async function browseModels(nextQuery = modelQuery, nextSort = modelSort) {
    setBrowserBusy(true);
    try {
      await savePendingHfToken();
      setModelResults(await searchHuggingFaceModels(nextQuery, nextSort, 20));
    } finally {
      setBrowserBusy(false);
    }
  }

  async function inspectModelFiles(selectedRepoId = repoId, selectedRevision = revision) {
    const repo = selectedRepoId.trim();
    const modelRevision = selectedRevision.trim();
    if (!repo) {
      setMessage('Repo ID is required');
      return;
    }
    setModelFileBusy(true);
    try {
      await savePendingHfToken();
      const inspection = await inspectHuggingFaceModel(repo, modelRevision || undefined);
      setModelFileDetails(inspection.ggufFileDetails);
      const selectedFile = preferredRunnableGguf([inspection.recommendedFile, ...inspection.ggufFiles]);
      if (selectedFile) {
        setFilename(selectedFile);
      }
      setPreflight(null);
      setDownloadPlan(null);
      setDownloadProgress(null);
      setDownloadFailed(false);
      setMessage(`Resolved ${inspection.ggufFiles.length} GGUF file(s) for ${repo}${modelRevision ? ` @ ${modelRevision}` : ''}`);
    } catch (error) {
      setMessage(`Could not inspect ${repo}: ${String(error)}`);
    } finally {
      setModelFileBusy(false);
    }
  }

  async function useModel(model: HuggingFaceModel) {
    setRepoId(model.repoId);
    setRevision('');
    setFilename(preferredRunnableGguf([model.recommendedFile, ...model.ggufFiles]));
    setModelFileDetails(model.ggufFiles.map(file => ({ file, sizeBytes: 0, sha256: null, quantization: null })));
    setPreflight(null);
    setDownloadPlan(null);
    setDownloadProgress(null);
    setDownloadFailed(false);
    setMessage(model.gated && !hf?.tokenAvailable
      ? `Selected gated model ${model.repoId}. Save a Hugging Face token before downloading.`
      : `Selected ${model.repoId}`);
    await inspectModelFiles(model.repoId, '');
  }

  function useDownloadedModel(model: DownloadedModel) {
    const selectedFile = preferredRunnableGguf([model.selectedFile, ...model.ggufFiles]);
    setRepoId(model.repoId);
    setRevision('');
    setFilename(selectedFile);
    setModelFileDetails(model.ggufFileDetails);
    setPreflight(null);
    setDownloadPlan(null);
    setDownloadProgress(null);
    setDownloadFailed(false);
    setMessage(`Selected downloaded model ${model.repoId}`);
  }

  function localServerSettings() {
    return validateLocalServerSettings(port, context);
  }

  function serverSettingsFromDownloadJob(job: HuggingFaceDownloadJob) {
    return validateLocalServerSettings(job.startPort ?? port, job.startContext ?? context);
  }

  function validateLocalServerSettings(nextPort: number, nextContext: number) {
    if (!Number.isInteger(nextPort) || nextPort < 1024 || nextPort > 65535) {
      throw new Error('Port must be an integer between 1024 and 65535');
    }
    if (!Number.isInteger(nextContext) || nextContext < 128 || nextContext > 131072) {
      throw new Error('Context must be an integer between 128 and 131072 tokens');
    }
    return { port: nextPort, context: nextContext };
  }

  function hfAutoBenchmarkTargetIds(selectedFilename: string, selectedRepoId: string, selectedPort: number, compareAfterStart = autoCompareAfterStart) {
    const repo = selectedRepoId.trim();
    if (!repo) {
      return [];
    }
    const targetIds = [hfLocalTargetId(repo, selectedFilename.trim(), selectedPort)];
    if (compareAfterStart && selectedAutoCompareCloudTarget) {
      targetIds.push(selectedAutoCompareCloudTarget.id);
    }
    return targetIds;
  }

  async function revealDownloadedModel(model: DownloadedModel) {
    try {
      await revealHuggingFaceModel(model.repoId);
      setMessage(`Opened ${model.repoId}`);
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function deleteDownloadedModel(model: DownloadedModel) {
    if (!window.confirm(`Delete downloaded files for ${model.repoId}?`)) {
      return;
    }
    setBusy(true);
    try {
      const status = await deleteHuggingFaceModel(model.repoId);
      setHf(status);
      if (repoId.trim() === model.repoId) {
        setRepoId('');
        setRevision('');
        setFilename('');
        setModelFileDetails([]);
        setPreflight(null);
        setDownloadPlan(null);
        setDownloadProgress(null);
        setDownloadFailed(false);
      }
      setMessage(`Deleted ${model.repoId}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function downloadModel() {
    setBusy(true);
    setDownloadLog('');
    setDownloadPlan(null);
    setDownloadProgress(null);
    setDownloadFailed(false);
    const modelRevision = revision.trim();
    setMessage(`Planning download for ${repoId}${modelRevision ? ` @ ${modelRevision}` : ''}${filename ? ` / ${filename}` : ''}. Large GGUF files can take several minutes after the disk check.`);
    const downloadId = crypto.randomUUID();
    try {
      await savePendingHfToken();
      const plan = await planHuggingFaceDownload(repoId, filename || undefined, modelRevision || undefined);
      setDownloadPlan(plan);
      setFilename(plan.selectedFile);
      setDownloadProgress(progressFromPlan(downloadId, plan));
      setDownloadLog([plan.summary, plan.diskCheck, plan.retryHint].filter(Boolean).join('\n'));
      setMessage(plan.alreadyDownloaded
        ? `Verifying existing ${plan.selectedFile}`
        : `Downloading ${plan.selectedFile}${plan.plannedBytes ? ` (${formatBytes(plan.plannedBytes)})` : ''}`);
      const settings = autoStartAfterDownload ? localServerSettings() : null;
      const compareWithCloud = autoCompareReady;
      const job = await startHuggingFaceDownloadJob(repoId, plan.selectedFile, modelRevision || undefined, {
        startAfterDownload: autoStartAfterDownload,
        runConnectivityAfterStart: autoBenchmarkAfterStart,
        autoBenchmarkPackId: autoBenchmarkAfterStart ? autoBenchmarkPackId : undefined,
        autoCompareAfterStart: compareWithCloud,
        autoBenchmarkTargetIds: autoStartAfterDownload && autoBenchmarkAfterStart
          ? hfAutoBenchmarkTargetIds(plan.selectedFile, repoId, settings?.port ?? port, compareWithCloud)
          : undefined,
        startPort: settings?.port,
        startContext: settings?.context,
      });
      if (autoStartAfterDownload) {
        autoStartDownloadJobIdsRef.current.add(job.id);
      }
      setDownloadJobs(current => mergeDownloadJob(current, job));
      setDownloadProgress(progressFromDownloadJob(job));
      if (isDownloadJobActive(job)) {
        setActiveDownloadJobId(job.id);
        const handoffNote = autoStartAfterDownload
          ? hfBenchmarkHandoffNote(autoBenchmarkAfterStart, autoBenchmarkPackId, modelBenchmarkPacks, compareWithCloud)
          : '';
        setMessage(`Started download job ${job.id.slice(0, 8)} for ${plan.selectedFile}${handoffNote}`);
      } else {
        await handleFinishedDownloadJob(job);
      }
    } catch (error) {
      setDownloadFailed(true);
      setDownloadProgress(previous => previous ? { ...previous, status: 'error', message: String(error) } : previous);
      setDownloadLog(String(error));
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function startModel() {
    setBusy(true);
    try {
      const settings = localServerSettings();
      const serverJobId = await startModelFromSelection(hf, settings);
      if (serverJobId) {
        setMessage(`Server start job ${serverJobId.slice(0, 8)} queued${hfBenchmarkHandoffNote(autoBenchmarkAfterStart, autoBenchmarkPackId, modelBenchmarkPacks, autoCompareReady)}`);
      }
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function cancelDownload(id: string) {
    setMessage(`Cancelling download job ${id.slice(0, 8)}`);
    try {
      const job = await cancelHuggingFaceDownloadJob(id);
      if (job) {
        setDownloadJobs(current => mergeDownloadJob(current, job));
        setDownloadProgress(progressFromDownloadJob(job));
        if (isDownloadJobActive(job)) {
          setActiveDownloadJobId(job.id);
        } else {
          await handleFinishedDownloadJob(job);
        }
      }
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function retryDownload(id: string) {
    setBusy(true);
    setDownloadFailed(false);
    setMessage(`Retrying download job ${id.slice(0, 8)}`);
    try {
      await savePendingHfToken();
      const job = await retryHuggingFaceDownloadJob(id);
      if (job) {
        if (job.startAfterDownload) {
          autoStartDownloadJobIdsRef.current.add(job.id);
        }
        setDownloadJobs(current => mergeDownloadJob(current, job));
        setDownloadProgress(progressFromDownloadJob(job));
        if (isDownloadJobActive(job)) {
          setActiveDownloadJobId(job.id);
        } else {
          await handleFinishedDownloadJob(job);
        }
      }
    } catch (error) {
      setDownloadFailed(true);
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function clearFinishedDownloads() {
    setBusy(true);
    try {
      const count = await clearFinishedHuggingFaceDownloadJobs();
      setDownloadJobs(current => current.filter(isDownloadJobActive));
      setMessage(`Cleared ${count} finished download jobs`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function cancelServerStart(id: string) {
    setMessage(`Cancelling server start job ${id.slice(0, 8)}`);
    try {
      const job = await cancelHuggingFaceServerJob(id);
      if (job) {
        setServerJobs(current => mergeServerJob(current, job));
        if (isServerJobActive(job)) {
          setActiveServerJobId(job.id);
        }
      }
    } catch (error) {
      setMessage(String(error));
    }
  }

  async function retryServerStart(id: string) {
    setBusy(true);
    setMessage(`Retrying server start job ${id.slice(0, 8)}`);
    try {
      const job = await retryHuggingFaceServerJob(id);
      if (job) {
        if (job.registerTargetAfterStart) {
          autoRegisterServerJobIdsRef.current.add(job.id);
        }
        setServerJobs(current => mergeServerJob(current, job));
        if (isServerJobActive(job)) {
          setActiveServerJobId(job.id);
        } else {
          await handleFinishedServerJob(job);
        }
      }
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function clearFinishedServerStarts() {
    setBusy(true);
    try {
      const count = await clearFinishedHuggingFaceServerJobs();
      setServerJobs(current => current.filter(isServerJobActive));
      setMessage(`Cleared ${count} finished server start jobs`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function startModelFromSelection(
    _status: HuggingFaceStatus | null,
    settings = localServerSettings(),
    selectedFilename = filename,
    selectedRepoId = repoId,
    runBenchmarkAfterStart = autoBenchmarkAfterStart,
    benchmarkPackId = autoBenchmarkPackId,
    compareAfterStart = autoCompareAfterStart,
  ): Promise<string | null> {
    const check = await runPreflight(settings, selectedFilename, selectedRepoId);
    if (check.errors.length) {
      throw new Error(`Preflight failed: ${check.errors.join(' ')}`);
    }
    if (check.warnings.length && !window.confirm(`Preflight warnings:\n\n${check.warnings.join('\n')}\n\nStart anyway?`)) {
      setMessage('Start cancelled after preflight warnings');
      return null;
    }
    const job = await startHuggingFaceServerJob(selectedRepoId, selectedFilename || undefined, settings.port, settings.context, {
      registerTargetAfterStart: true,
      runConnectivityAfterStart: runBenchmarkAfterStart,
      autoBenchmarkPackId: runBenchmarkAfterStart ? benchmarkPackId : undefined,
      autoCompareAfterStart: runBenchmarkAfterStart && compareAfterStart && Boolean(selectedAutoCompareCloudTarget),
      autoBenchmarkTargetIds: runBenchmarkAfterStart
        ? hfAutoBenchmarkTargetIds(selectedFilename, selectedRepoId, settings.port, compareAfterStart)
        : undefined,
    });
    autoRegisterServerJobIdsRef.current.add(job.id);
    setServerJobs(current => mergeServerJob(current, job));
    setActiveServerJobId(job.id);
    if (!isServerJobActive(job)) {
      await handleFinishedServerJob(job);
    }
    return job.id;
  }

  async function runPreflight(settings = localServerSettings(), selectedFilename = filename, selectedRepoId = repoId) {
    setPreflightBusy(true);
    try {
      await savePendingHfToken();
      const result = await preflightHuggingFaceModel(selectedRepoId, selectedFilename || undefined, settings.context);
      setPreflight(result);
      setMessage(`Preflight ${result.status}: ${result.summary}`);
      return result;
    } finally {
      setPreflightBusy(false);
    }
  }

  async function stopModel() {
    setBusy(true);
    try {
      setHf(await stopHuggingFaceModel());
      setMessage('Local model server stopped');
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function addLocalTarget() {
    setBusy(true);
    try {
      const target = await registerLocalTarget(hf, localServerSettings());
      const validation = await validateTarget(target.id);
      await finishLocalTargetHandoff(target, validation, 'Local target saved', autoBenchmarkAfterStart, autoBenchmarkPackId, autoCompareAfterStart);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }

  function hfBenchmarkIntentForTarget(target: Target, benchmarkPackId: string, allowComparison: boolean, compareAfterStart: boolean) {
    const localIntent = automaticModelRunBuilderIntent(target, benchmarkPackId);
    if (!allowComparison || !compareAfterStart || !selectedAutoCompareCloudTarget) {
      return localIntent;
    }
    const comparisonTargets = [target, selectedAutoCompareCloudTarget];
    return modelComparisonIntentForTarget(target, comparisonTargets, benchmarkPackId) ?? localIntent;
  }

  async function finishLocalTargetHandoff(
    target: Target,
    validation: TargetValidation,
    prefix: string,
    runBenchmarkAfterStart = autoBenchmarkAfterStart,
    benchmarkPackId = autoBenchmarkPackId,
    compareAfterStart = autoCompareAfterStart,
  ) {
    await refresh();
    if (validation.status === 'error') {
      setMessage(`${prefix}, but validation failed: ${validation.detail}`);
      return;
    }
    const validationNote = validation.status === 'ok' ? 'validated' : `saved with warning: ${validation.detail}`;
    const packId = benchmarkPackId || 'llm-basics';
    const packLabel = benchmarkPackLabel(packId, modelBenchmarkPacks);
    const intent = hfBenchmarkIntentForTarget(target, packId, runBenchmarkAfterStart, compareAfterStart);
    const scopeLabel = intent.targetIds.length > 1 ? 'local/cloud comparison' : 'local benchmark';
    if (!runBenchmarkAfterStart) {
      openRunBuilder(intent);
      setMessage(`Local target ${target.name} ${validationNote} and ready in Runs`);
      return;
    }
    try {
      const automaticSettings = automaticModelBenchmarkSettings(packId);
      const job = await startRunJob(
        intent.targetIds,
        false,
        packId,
        intent.repetitions ?? automaticSettings.repetitions,
        intent.warmupRuns ?? automaticSettings.warmupRuns,
        intent.concurrency ?? automaticSettings.concurrency,
        intent.maxCostUsd ?? automaticModelBenchmarkMaxCostUsd(packId),
      );
      await refresh();
      if (!isJobActive(job) && job.results.length) {
        openResultsForGroup(job.runGroupId, job.results[0]?.id);
        setMessage(`Local target ${target.name} ${validationNote}; capped ${packLabel} ${scopeLabel} completed with ${job.results.length} result(s)`);
        return;
      }
      openRunBuilder(intent);
      setMessage(`Local target ${target.name} ${validationNote}; queued capped ${packLabel} ${scopeLabel} job ${job.id.slice(0, 8)}`);
    } catch (error) {
      openRunBuilder(intent);
      setMessage(`Local target ${target.name} ${validationNote}, but the automatic ${packLabel} ${scopeLabel} job could not start: ${benchmarkRunFailureMessage(error)}`);
    }
  }

  async function registerLocalTarget(status: HuggingFaceStatus | null, settings = localServerSettings(), selectedFilename = filename, selectedRepoId = repoId): Promise<Target> {
    const repo = selectedRepoId.trim();
    if (!repo) {
      throw new Error('Repo ID is required');
    }
    const downloaded = status?.models.find(model => model.repoId === repo);
    const selectedFile = selectedFilename.trim()
      || preferredRunnableGguf([downloaded?.selectedFile, ...(downloaded?.ggufFiles ?? [])])
      || '';
    if (selectedRepoId === repoId && selectedFile && !filename.trim()) {
      setFilename(selectedFile);
    }
    const servedModel = status?.serverModelId?.trim() || selectedFile || repo;
    const config: Record<string, unknown> = {
      model: servedModel,
      base_url: `http://127.0.0.1:${settings.port}/v1`,
      source: 'huggingface-local',
      repo_id: repo,
      port: settings.port,
      context: settings.context,
      temperature: 0,
      top_p: 1,
      max_tokens: hfLocalTargetMaxTokens(settings.context),
      timeout_seconds: 120,
      retry_count: 1,
      input_price_usd_per_million_tokens: 0,
      output_price_usd_per_million_tokens: 0,
    };
    if (selectedFile) {
      config.gguf_file = selectedFile;
    }
    if (downloaded?.path) {
      config.model_path = downloaded.path;
    }
    if (downloaded?.revision?.trim()) {
      config.revision = downloaded.revision.trim();
    }
    const fileLabel = selectedFile ? ` ${selectedFile.replace(/\.gguf$/i, '')}` : '';
    const name = `HF Local ${repo}${fileLabel}`;
    return createTarget({
      id: hfLocalTargetId(repo, selectedFile, settings.port),
      name,
      kind: 'direct_model',
      adapterId: 'llama-cpp-openai',
      config,
    });
  }

  async function startWorker() {
    setBusy(true);
    try {
      const result = await runWorkerMock();
      await refresh();
      setMessage(`Worker check ${result.status}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  return <section><div className="section-head"><h1>Settings</h1><button disabled={busy} onClick={startWorker}><Play size={16} />Worker check</button></div>
    <div className="panel compact">
      <h2>Hugging Face Local Model</h2>
      <div className="status-list">
        <span><span className={`pill ${hf?.tokenAvailable ? 'ok' : 'warn'}`}>token</span></span>
        <span title={hfPythonSetupDetail(hf)}><span className={`pill ${hfPythonReadyForSetup(hf) ? 'ok' : 'warn'}`}>python</span></span>
        <span><span className={`pill ${hf?.hfCliAvailable ? 'ok' : 'warn'}`}>hf</span></span>
        <span><span className={`pill ${hf?.llamaServerAvailable ? 'ok' : 'warn'}`}>llama-server</span></span>
        <span><span className={`pill ${hf?.serverRunning ? 'ok' : 'warn'}`}>server</span></span>
      </div>
      <p className="muted">{hf?.detail ?? 'Checking Hugging Face setup'}</p>
      {hf ? <div className="preflight-box">
        <div className="panel-head"><h2>Model cache</h2><button title="Copy model cache path" onClick={() => copyHfCachePath().catch(error => setMessage(String(error)))}><Copy size={14} />Path</button></div>
        <div className="mini-grid">
          <span>{hf.models.length} repos</span>
          <span>{downloadedGgufCount} GGUF files</span>
          <span>{formatBytes(hf.cacheSizeBytes)} stored</span>
          <span>{hf.cacheFreeBytes != null ? `${formatBytes(hf.cacheFreeBytes)} free` : 'free space unknown'}</span>
        </div>
        <p className="muted">{hf.cacheDir}</p>
        {activeDownloadCount || activeServerStartCount ? <p className="muted">{activeDownloadCount} active download job(s), {activeServerStartCount} active server-start job(s)</p> : null}
      </div> : null}
      <button disabled={busy || Boolean(hf?.hfCliAvailable && hf?.llamaServerAvailable)} onClick={installMissingTools}><Wrench size={16} />Install missing tools</button>
      {installLog ? <pre className="setup-log">{installLog}</pre> : null}
      <div className="preflight-box hf-handoff-box">
        <div className="panel-head"><h2>Automatic handoff</h2><span className={`pill ${hfHandoffReady ? 'ok' : 'warn'}`}>{hfHandoffReady ? 'ready' : 'needs input'}</span></div>
        <div className="handoff-track">
          {hfHandoffSteps.map(step => <span key={step.label} className={`handoff-step ${step.tone}`}>
            <strong>{step.label}</strong>
            <small>{step.detail}</small>
          </span>)}
        </div>
        <div className="handoff-controls">
          <label className="toggle"><input type="checkbox" checked={autoStartAfterDownload} onChange={event => setAutoStartAfterDownload(event.target.checked)} />Start after download</label>
          <label className="toggle"><input type="checkbox" checked={autoBenchmarkAfterStart} onChange={event => setAutoBenchmarkAfterStart(event.target.checked)} />Run benchmark after start</label>
          <label className="toggle" title={selectablePricedCloudTargetCount ? 'Include the selected priced cloud target in the automatic benchmark' : selectableCloudTargetCount ? 'Add input/output pricing to a cloud target before automatic local/cloud comparison' : 'Add a selectable cloud target before automatic local/cloud comparison'}>
            <input
              type="checkbox"
              checked={autoCompareAfterStart}
              disabled={!autoBenchmarkAfterStart}
              onChange={event => setAutoCompareAfterStart(event.target.checked)}
            />
            Compare with cloud target
          </label>
          {autoBenchmarkAfterStart && autoCompareAfterStart && !selectablePricedCloudTargetCount ? <div className="handoff-remedy">
            <span className="mini-tag warn">{selectableCloudTargetCount ? 'pricing needed' : 'cloud target needed'}</span>
            <button type="button" title="Run the selected benchmark on the local model now; cloud comparison can be added later" onClick={useLocalOnlyHandoff}><Play size={14} />Local only</button>
            <button type="button" title={cloudComparisonActionTitle} onClick={openCloudComparisonSetup}>{cloudComparisonNeedsPricing ? <Pencil size={14} /> : <Boxes size={14} />}{cloudComparisonActionLabel}</button>
          </div> : null}
          {selectablePricedCloudTargets.length ? <label>Cloud target <select value={selectedAutoCompareCloudTarget?.id ?? ''} disabled={!autoBenchmarkAfterStart || !autoCompareAfterStart} onChange={event => setAutoCompareCloudTargetId(event.target.value)}>
            {selectablePricedCloudTargets.map(target => <option key={target.id} value={target.id}>{hfCloudTargetLabel(target)}</option>)}
          </select></label> : null}
          <label>Benchmark pack <select value={autoBenchmarkPackId} disabled={!autoBenchmarkAfterStart} onChange={event => setAutoBenchmarkPackId(event.target.value)}>{modelBenchmarkPacks.map(pack => <option key={pack.id} value={pack.id}>{pack.label}</option>)}</select></label>
        </div>
      </div>
      <h2>Browse Hub Models</h2>
      <div className="browser-controls">
        <input value={modelQuery} onChange={event => setModelQuery(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') browseModels().catch(error => setMessage(String(error))); }} placeholder="Search GGUF models, e.g. Qwen, Gemma, Mistral" />
        <select value={modelSort} onChange={event => { setModelSort(event.target.value); browseModels(modelQuery, event.target.value).catch(error => setMessage(String(error))); }}>
          <option value="trendingScore">Trending</option>
          <option value="downloads">Downloads</option>
          <option value="likes">Likes</option>
          <option value="lastModified">Recent</option>
        </select>
        <button disabled={browserBusy} onClick={() => browseModels().catch(error => setMessage(String(error)))}><Search size={16} />Search</button>
      </div>
      {modelResults.length ? <div className="model-browser">
        {modelResults.map(model => <div className="model-row" key={model.repoId}>
          <div className="model-main">
            <strong>{model.repoId}</strong>
            <span className="muted">{model.pipelineTag ?? model.libraryName ?? 'model'} · {formatCount(model.downloads)} downloads · {formatCount(model.likes)} likes{model.gated ? ' · gated' : ''}</span>
            <span className="muted">{model.recommendedFile ?? model.ggufFiles[0] ?? 'GGUF files listed on model page'}</span>
            <div className="tag-row">{model.tags.filter(tag => !tag.includes(':')).slice(0, 5).map(tag => <span className="mini-tag" key={`${model.repoId}-${tag}`}>{tag}</span>)}</div>
          </div>
          <div className="model-actions">
            <button disabled={busy || modelFileBusy} onClick={() => useModel(model).catch(error => setMessage(String(error)))}>{modelFileBusy && repoId === model.repoId ? 'Resolving' : 'Use'}</button>
            <button onClick={() => window.open(`${model.url}?local-app=llama.cpp`, '_blank')}><ExternalLink size={16} />HF</button>
          </div>
        </div>)}
      </div> : <p className="muted">{browserBusy ? 'Searching Hugging Face...' : 'No Hub models loaded yet.'}</p>}
      <div className="form-grid">
        <label>HF token <input type="password" value={token} onChange={event => setToken(event.target.value)} placeholder="hf_..." /></label>
        <button disabled={busy || !token.trim()} onClick={saveToken}>Save token</button>
        <label>Repo ID <input value={repoId} onChange={event => { setRepoId(event.target.value); setModelFileDetails([]); setPreflight(null); setDownloadPlan(null); setDownloadProgress(null); setDownloadFailed(false); }} placeholder="org/model-GGUF" /></label>
        <label>GGUF file {modelFileDetails.length ? <select value={filename} onChange={event => { setFilename(event.target.value); setPreflight(null); setDownloadPlan(null); setDownloadProgress(null); setDownloadFailed(false); }}><option value="">Auto-select</option>{modelFileDetails.filter(detail => isRunnableGgufFile(detail.file)).map(detail => <option key={detail.file} value={detail.file}>{detail.file}{detail.sizeBytes ? ` (${formatBytes(detail.sizeBytes)})` : ''}</option>)}</select> : <input value={filename} onChange={event => { setFilename(event.target.value); setPreflight(null); setDownloadPlan(null); setDownloadProgress(null); setDownloadFailed(false); }} placeholder="optional, e.g. model-q4_k_m.gguf" />}</label>
        <button disabled={busy || modelFileBusy || !repoId.trim()} onClick={() => inspectModelFiles().catch(error => setMessage(String(error)))}><Search size={16} />{modelFileBusy ? 'Resolving' : 'Files'}</button>
        <button disabled={busy || activeDownloadInProgress || activeServerStartInProgress || !repoId.trim()} onClick={downloadModel}>{downloadFailed ? <RotateCcw size={16} /> : <Download size={16} />}{hfDownloadActionLabel(downloadFailed, autoStartAfterDownload, autoBenchmarkAfterStart, autoCompareReady)}</button>
        {hf?.serverRunning ? <button disabled={busy || activeServerStartInProgress} onClick={stopModel}>Stop</button> : null}
        <details className="advanced-section" open={localAdvancedOpen} onToggle={event => setLocalAdvancedOpen(event.currentTarget.open)}>
          <summary><SlidersHorizontal size={14} />Advanced</summary>
          <div className="form-grid">
            <label>Revision <input value={revision} onChange={event => { setRevision(event.target.value); setModelFileDetails([]); setPreflight(null); setDownloadPlan(null); setDownloadProgress(null); setDownloadFailed(false); }} placeholder="main, tag, branch, or commit" /></label>
            <label>Port <input type="number" min="1024" max="65535" step="1" value={port} onChange={event => setPort(Number(event.target.value))} /></label>
            <label>Context <input type="number" min="128" max="131072" step="1" value={context} onChange={event => { setContext(Number(event.target.value)); setPreflight(null); }} /></label>
            <button disabled={busy || preflightBusy || !repoId.trim()} onClick={() => runPreflight().catch(error => setMessage(String(error)))}><ShieldCheck size={16} />{preflightBusy ? 'Checking' : 'Check'}</button>
            <button disabled={busy || activeServerStartInProgress || !repoId.trim()} onClick={startModel}><Play size={16} />{hfStartActionLabel(autoBenchmarkAfterStart, autoCompareReady)}</button>
            <button disabled={busy || activeServerStartInProgress || !hf?.serverRunning || !repoId.trim()} onClick={addLocalTarget}><Plus size={16} />Add target</button>
          </div>
        </details>
      </div>
      {downloadPlan ? <div className={`preflight-box ${downloadPlan.alreadyDownloaded ? 'ok' : downloadFailed ? 'warn' : ''}`}><strong>Download plan</strong><p>{downloadPlan.summary}</p><div className="mini-grid"><span>{downloadPlan.plannedBytes ? formatBytes(downloadPlan.plannedBytes) : 'size unknown'}</span><span>{downloadPlan.existingBytes ? `${formatBytes(downloadPlan.existingBytes)} local` : 'no complete local file'}</span><span>{downloadPlan.partialBytes ? `${formatBytes(downloadPlan.partialBytes)} partial` : 'no partial fragments'}</span><span>{downloadPlan.alreadyDownloaded ? 'already downloaded' : 'needs transfer'}</span></div><p className="muted">{downloadPlan.diskCheck} {downloadPlan.retryHint}</p><p className="muted">{downloadPlan.localDir}</p></div> : null}
      {autoBenchmarkAfterStart && autoCompareAfterStart && !autoCompareReady ? <div className="preflight-box warn">
        <div className="panel-head"><h2>Cloud comparison</h2><div className="actions"><button title="Run the selected benchmark on the local model now; cloud comparison can be added later" onClick={useLocalOnlyHandoff}><Play size={14} />Local only</button><button title={cloudComparisonActionTitle} onClick={openCloudComparisonSetup}>{cloudComparisonNeedsPricing ? <Pencil size={14} /> : <Boxes size={14} />}{cloudComparisonActionLabel}</button></div></div>
        <p>{cloudComparisonStatus} The local model flow will still create a target and run the selected pack locally.</p>
      </div> : null}
      {downloadProgress ? <DownloadProgressPanel progress={downloadProgress} /> : null}
      {downloadJobs.length ? <DownloadJobsTable jobs={downloadJobs} busy={busy} onCancel={cancelDownload} onRetry={retryDownload} onClearFinished={clearFinishedDownloads} /> : null}
      {serverJobs.length ? <ServerJobsTable jobs={serverJobs} busy={busy} onCancel={cancelServerStart} onRetry={retryServerStart} onClearFinished={clearFinishedServerStarts} /> : null}
      {preflight ? <div className={`preflight-box ${preflight.status}`}><strong>Preflight {preflight.status}</strong><p>{preflight.summary}</p><div className="mini-grid"><span>Model {formatBytes(preflight.modelSizeBytes)}</span><span>Estimated {formatBytes(preflight.estimatedMemoryBytes)}</span><span>System {preflight.systemMemoryBytes ? formatBytes(preflight.systemMemoryBytes) : 'unknown'}</span><span>Context {preflight.context}</span></div>{preflight.errors.length ? <ul>{preflight.errors.map(item => <li key={item}>{item}</li>)}</ul> : null}{preflight.warnings.length ? <ul>{preflight.warnings.map(item => <li key={item}>{item}</li>)}</ul> : null}</div> : null}
      {downloadLog ? <pre className="setup-log">{downloadLog}</pre> : null}
      {hf?.models.length ? <table><thead><tr><th>Downloaded model</th><th>Size</th><th>GGUF files</th><th></th></tr></thead><tbody>{hf.models.map(model => <tr key={model.path}><td><strong>{model.repoId}</strong><div className="muted">{model.path}</div>{model.selectedFile ? <div className="mini-tag">selected {model.selectedFile}</div> : null}</td><td>{formatBytes(model.sizeBytes)}</td><td><GgufFileList model={model} /></td><td><div className="row-actions"><button disabled={busy} onClick={() => useDownloadedModel(model)}>Use</button><button disabled={busy} onClick={() => revealDownloadedModel(model)}><ExternalLink size={14} />Reveal</button><button disabled={busy} onClick={() => deleteDownloadedModel(model)}><Trash2 size={14} />Delete</button></div></td></tr>)}</tbody></table> : null}
    </div>
    <div className="panel compact"><h2>Secrets</h2><p>Hugging Face tokens are stored in macOS Keychain. Adapter exports redact keys, tokens, passwords, and secret fields.</p></div>
  </section>;
}

function hfPythonReadyForSetup(hf: HuggingFaceStatus | null) {
  return Boolean(hf?.hfCliAvailable || hf?.pythonSupported);
}

function hfAutomaticHandoffSteps({
  repoId,
  selectedFile,
  hf,
  autoStartAfterDownload,
  autoBenchmarkAfterStart,
  autoCompareAfterStart,
  autoBenchmarkPackId,
  modelBenchmarkPacks,
  selectableCloudTargetCount,
  selectablePricedCloudTargetCount,
  selectedCloudTargetLabel,
  port,
  context,
}: {
  repoId: string;
  selectedFile: string;
  hf: HuggingFaceStatus | null;
  autoStartAfterDownload: boolean;
  autoBenchmarkAfterStart: boolean;
  autoCompareAfterStart: boolean;
  autoBenchmarkPackId: string;
  modelBenchmarkPacks: BenchmarkPackOption[];
  selectableCloudTargetCount: number;
  selectablePricedCloudTargetCount: number;
  selectedCloudTargetLabel: string;
  port: number;
  context: number;
}): HfAutomaticHandoffStep[] {
  const steps: HfAutomaticHandoffStep[] = [
    {
      label: 'Download',
      tone: repoId ? 'ok' : 'unknown',
      detail: repoId ? `${repoId}${selectedFile ? ` / ${selectedFile}` : ' / auto-select GGUF'}` : 'Choose a GGUF repository',
    },
  ];
  if (!autoStartAfterDownload) {
    steps.push(
      {
        label: 'Start',
        tone: 'unknown',
        detail: 'Manual server start',
        optional: true,
      },
      {
        label: 'Target',
        tone: 'unknown',
        detail: 'Add after the server is running',
        optional: true,
      },
    );
  } else {
    const llamaReady = Boolean(hf?.llamaServerAvailable);
    steps.push(
      {
        label: 'Start',
        tone: llamaReady ? 'ok' : 'warn',
        detail: llamaReady ? `llama-server on port ${port}, ctx ${context}` : 'Install llama.cpp first',
      },
      {
        label: 'Target',
        tone: llamaReady ? 'ok' : 'warn',
        detail: llamaReady ? 'Register and validate local target' : 'Waiting for llama-server',
      },
    );
  }

  if (!autoBenchmarkAfterStart) {
    steps.push({
      label: 'Benchmark',
      tone: 'unknown',
      detail: 'Manual run after target creation',
      optional: true,
    });
  } else {
    steps.push({
      label: 'Benchmark',
      tone: autoStartAfterDownload ? 'ok' : 'warn',
      detail: benchmarkPackLabel(autoBenchmarkPackId, modelBenchmarkPacks),
    });
  }

  if (!autoBenchmarkAfterStart || !autoCompareAfterStart) {
    steps.push({
      label: 'Cloud',
      tone: 'unknown',
      detail: 'Local-only handoff',
      optional: true,
    });
  } else if (selectablePricedCloudTargetCount > 0) {
    steps.push({
      label: 'Cloud',
      tone: 'ok',
      detail: selectedCloudTargetLabel || `${selectablePricedCloudTargetCount} priced target(s) available`,
    });
  } else if (selectableCloudTargetCount > 0) {
    steps.push({
      label: 'Cloud',
      tone: 'warn',
      detail: 'Add input/output pricing',
    });
  } else {
    steps.push({
      label: 'Cloud',
      tone: 'warn',
      detail: 'Add a cloud target',
    });
  }
  return steps;
}

function hfDownloadActionLabel(downloadFailed: boolean, autoStartAfterDownload: boolean, autoBenchmarkAfterStart: boolean, compareWithCloud = false) {
  if (downloadFailed) {
    return 'Retry download';
  }
  if (autoStartAfterDownload && autoBenchmarkAfterStart && compareWithCloud) {
    return 'Download + compare';
  }
  if (autoStartAfterDownload && autoBenchmarkAfterStart) {
    return 'Download + benchmark';
  }
  if (autoStartAfterDownload) {
    return 'Download & start';
  }
  return 'Download';
}

function hfStartActionLabel(autoBenchmarkAfterStart: boolean, compareWithCloud = false) {
  if (autoBenchmarkAfterStart && compareWithCloud) {
    return 'Start + compare';
  }
  return autoBenchmarkAfterStart ? 'Start + benchmark' : 'Start & add target';
}

function hfCloudComparisonStatus(cloudTargetCount: number, pricedCloudTargetCount: number) {
  if (pricedCloudTargetCount > 0) {
    return `${pricedCloudTargetCount} priced cloud target(s) ready for capped comparison.`;
  }
  if (cloudTargetCount > 0) {
    return 'Cloud comparison is waiting for input and output pricing on a cloud target.';
  }
  return 'Cloud comparison is waiting for a selectable cloud target.';
}

function hfCloudTargetLabel(target: Target) {
  return [target.name, target.model || target.id].filter(Boolean).join(' / ');
}

function hfBenchmarkHandoffNote(runBenchmarkAfterStart: boolean, benchmarkPackId: string, modelBenchmarkPacks: BenchmarkPackOption[], compareWithCloud = false) {
  if (!runBenchmarkAfterStart) {
    return '';
  }
  const scope = compareWithCloud ? ' local/cloud comparison' : '';
  return `; will queue ${benchmarkPackLabel(benchmarkPackId, modelBenchmarkPacks)}${scope}`;
}

function hfPythonSetupDetail(hf: HuggingFaceStatus | null) {
  if (!hf) {
    return 'Checking python3 for Hugging Face CLI setup';
  }
  if (hf.hfCliAvailable) {
    return hf.pythonVersion ? `hf is installed; python3 ${hf.pythonVersion} detected` : 'hf is installed; python3 is not required for setup';
  }
  if (hf.pythonSupported) {
    return `python3 ${hf.pythonVersion ?? ''} is ready for the Hugging Face CLI installer`.trim();
  }
  return hf.pythonVersion
    ? `python3 ${hf.pythonVersion} is too old for the Hugging Face CLI installer`
    : 'python3 was not found; Python 3.10+ is required for the Hugging Face CLI installer';
}

function GgufFileList({ model }: { model: DownloadedModel }) {
  const details = model.ggufFileDetails.length
    ? model.ggufFileDetails
    : model.ggufFiles.map(file => ({ file, sizeBytes: 0, sha256: null, quantization: null }));
  if (!details.length) {
    return <span className="muted">{model.files.length} files</span>;
  }
  return <div className="artifact-list compact-list">{details.slice(0, 3).map(detail => <div key={detail.file} className="compact-line"><strong>{detail.file}</strong><span className="muted">{ggufDetailText(detail)}</span></div>)}{details.length > 3 ? <span className="muted">and {details.length - 3} more</span> : null}</div>;
}

function DownloadProgressPanel({ progress }: { progress: HuggingFaceDownloadProgress }) {
  const percent = downloadProgressPercent(progress);
  return <div className={`preflight-box ${progress.status === 'completed' ? 'ok' : progress.status === 'error' ? 'error' : ''}`}>
    <div className="download-progress-head"><strong>{progress.selectedFile}</strong><span className={`pill ${progress.status === 'completed' ? 'ok' : progress.status === 'error' ? 'error' : 'warn'}`}>{progress.status}</span></div>
    <p>{progress.message}</p>
    <div className="progress-cell download-progress">
      <div className="progress-track"><div className="progress-fill" style={{ width: `${percent ?? 0}%` }} /></div>
      <span>{percent == null ? 'size unknown' : `${percent.toFixed(0)}%`}</span>
    </div>
    <div className="mini-grid">
      <span>{formatBytes(progress.transferredBytes)} transferred</span>
      <span>{progress.plannedBytes ? `${formatBytes(progress.plannedBytes)} planned` : 'planned size unknown'}</span>
      <span>{progress.repoId}</span>
      <span>{progress.localDir}</span>
    </div>
  </div>;
}

function DownloadJobsTable({ jobs, busy, onCancel, onRetry, onClearFinished }: { jobs: HuggingFaceDownloadJob[]; busy: boolean; onCancel: (id: string) => Promise<void>; onRetry: (id: string) => Promise<void>; onClearFinished: () => Promise<void> }) {
  return <div>
    <div className="panel-head"><h2>Download Jobs</h2><button disabled={busy || !jobs.some(isDownloadJobFinished)} onClick={() => onClearFinished()}><Trash2 size={14} />Clear finished</button></div>
    <table><thead><tr><th>Job</th><th>Repo</th><th>File</th><th>Status</th><th>Progress</th><th>Message</th><th>Started</th><th></th></tr></thead><tbody>{jobs.map(job => {
      const percent = downloadJobPercent(job);
      return <tr key={job.id}><td>{job.id.slice(0, 8)}</td><td>{job.repoId}</td><td>{job.selectedFile ?? '-'}</td><td><span className={`pill ${jobStatusClass(job.status)}`}>{job.status}</span></td><td><div className="progress-cell"><div className="progress-track"><div className="progress-fill" style={{ width: `${percent ?? 0}%` }} /></div><span>{percent == null ? formatBytes(job.transferredBytes) : `${percent.toFixed(0)}%`}</span></div></td><td><JobMessageCell job={job} /></td><td>{formatDateTime(job.startedAt)}</td><td><div className="row-actions">{isDownloadJobActive(job) ? <button disabled={job.status === 'cancelling'} title="Cancel download job" onClick={() => onCancel(job.id)}><Square size={14} />Stop</button> : isDownloadJobRetryable(job) ? <button disabled={busy} title="Retry download job" onClick={() => onRetry(job.id)}><RotateCcw size={14} />Retry</button> : null}</div></td></tr>;
    })}</tbody></table>
  </div>;
}

function ServerJobsTable({ jobs, busy, onCancel, onRetry, onClearFinished }: { jobs: HuggingFaceServerJob[]; busy: boolean; onCancel: (id: string) => Promise<void>; onRetry: (id: string) => Promise<void>; onClearFinished: () => Promise<void> }) {
  return <div>
    <div className="panel-head"><h2>Server Start Jobs</h2><button disabled={busy || !jobs.some(isServerJobFinished)} onClick={() => onClearFinished()}><Trash2 size={14} />Clear finished</button></div>
    <table><thead><tr><th>Job</th><th>Repo</th><th>File</th><th>Port</th><th>Status</th><th>Message</th><th>Started</th><th></th></tr></thead><tbody>{jobs.map(job => (
      <tr key={job.id}><td>{job.id.slice(0, 8)}</td><td>{job.repoId}</td><td>{job.selectedFile ?? '-'}</td><td>{job.port}</td><td><span className={`pill ${jobStatusClass(job.status)}`}>{job.status}</span></td><td><JobMessageCell job={job} /></td><td>{formatDateTime(job.startedAt)}</td><td><div className="row-actions">{isServerJobActive(job) ? <button disabled={job.status === 'cancelling'} title="Cancel server start job" onClick={() => onCancel(job.id)}><Square size={14} />Stop</button> : isServerJobRetryable(job) ? <button disabled={busy} title="Retry server start job" onClick={() => onRetry(job.id)}><RotateCcw size={14} />Retry</button> : null}</div></td></tr>
    ))}</tbody></table>
  </div>;
}

function progressFromPlan(downloadId: string, plan: HuggingFaceDownloadPlan): HuggingFaceDownloadProgress {
  const transferredBytes = Math.max(plan.existingBytes ?? 0, plan.partialBytes ?? 0);
  return {
    downloadId,
    repoId: plan.repoId,
    selectedFile: plan.selectedFile,
    status: 'planned',
    message: plan.alreadyDownloaded ? 'Existing file will be verified.' : plan.summary,
    localDir: plan.localDir,
    transferredBytes,
    plannedBytes: plan.plannedBytes,
    percent: plan.plannedBytes && plan.plannedBytes > 0 ? Math.min(100, (transferredBytes / plan.plannedBytes) * 100) : null,
  };
}

function progressFromDownloadJob(job: HuggingFaceDownloadJob): HuggingFaceDownloadProgress {
  const status = job.status === 'completed'
    ? 'completed'
    : job.status === 'failed'
      ? 'error'
      : job.status === 'cancelled'
        ? 'cancelled'
        : 'running';
  return {
    downloadId: job.id,
    repoId: job.repoId,
    selectedFile: job.selectedFile ?? 'resolving GGUF file',
    status,
    message: job.message,
    localDir: job.localDir ?? '',
    transferredBytes: job.transferredBytes,
    plannedBytes: job.plannedBytes,
    percent: job.percent,
  };
}

function downloadProgressPercent(progress: HuggingFaceDownloadProgress) {
  if (typeof progress.percent === 'number' && Number.isFinite(progress.percent)) {
    return Math.max(0, Math.min(100, progress.percent));
  }
  if (progress.plannedBytes && progress.plannedBytes > 0) {
    return Math.max(0, Math.min(100, (progress.transferredBytes / progress.plannedBytes) * 100));
  }
  return null;
}

function mergeDownloadJob(jobs: HuggingFaceDownloadJob[], next: HuggingFaceDownloadJob) {
  const existing = jobs.filter(job => job.id !== next.id);
  return [next, ...existing].sort((a, b) => b.startedAt.localeCompare(a.startedAt));
}

function isDownloadJobActive(job: HuggingFaceDownloadJob) {
  return job.status === 'queued' || job.status === 'running' || job.status === 'cancelling';
}

function isDownloadJobFinished(job: HuggingFaceDownloadJob) {
  return job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled';
}

function isDownloadJobRetryable(job: HuggingFaceDownloadJob) {
  return job.status === 'failed' || job.status === 'cancelled';
}

function mergeServerJob(jobs: HuggingFaceServerJob[], next: HuggingFaceServerJob) {
  const existing = jobs.filter(job => job.id !== next.id);
  return [next, ...existing].sort((a, b) => b.startedAt.localeCompare(a.startedAt));
}

function isServerJobActive(job: HuggingFaceServerJob) {
  return job.status === 'queued' || job.status === 'running' || job.status === 'cancelling';
}

function isServerJobFinished(job: HuggingFaceServerJob) {
  return job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled';
}

function isServerJobRetryable(job: HuggingFaceServerJob) {
  return job.status === 'failed' || job.status === 'cancelled';
}

function newestJob<T extends { startedAt: string }>(jobs: T[]) {
  return [...jobs].sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0];
}

function serverJobMatchesDownload(serverJob: HuggingFaceServerJob, downloadJob: HuggingFaceDownloadJob) {
  const selectedFile = downloadJob.model?.selectedFile ?? downloadJob.selectedFile ?? '';
  return serverJob.repoId === downloadJob.repoId
    && (selectedFile ? serverJob.selectedFile === selectedFile : true)
    && (downloadJob.startPort == null || serverJob.port === downloadJob.startPort)
    && (downloadJob.startContext == null || serverJob.context === downloadJob.startContext);
}

function hfLocalTargetId(repoId: string, selectedFile: string | null | undefined, port: number) {
  return slugify(`hf-local-${repoId}-${selectedFile || 'model'}-${port}`);
}

function hfLocalTargetMaxTokens(context: number) {
  if (!Number.isFinite(context) || context <= 0) {
    return 512;
  }
  const divisor = context <= 1024 ? 8 : 4;
  return Math.max(16, Math.min(512, Math.floor(context / divisor)));
}

function downloadJobPercent(job: HuggingFaceDownloadJob) {
  if (typeof job.percent === 'number' && Number.isFinite(job.percent)) {
    return Math.max(0, Math.min(100, job.percent));
  }
  if (job.plannedBytes && job.plannedBytes > 0) {
    return Math.max(0, Math.min(100, (job.transferredBytes / job.plannedBytes) * 100));
  }
  return null;
}

function ggufDetailText(detail: { sizeBytes: number; sha256?: string | null; quantization?: string | null }) {
  return [
    detail.sizeBytes ? formatBytes(detail.sizeBytes) : null,
    detail.quantization,
    detail.sha256 ? shortHash(detail.sha256) : null,
  ].filter(Boolean).join(' · ') || 'metadata unavailable';
}

function preferredRunnableGguf(files: Array<string | null | undefined>) {
  return files.find((file): file is string => Boolean(file && isRunnableGgufFile(file))) ?? '';
}

function isRunnableGgufFile(file: string) {
  const lower = file.toLowerCase();
  return lower.endsWith('.gguf') && !lower.includes('mmproj') && !lower.includes('projector');
}

function formatCount(value: number) {
  return Intl.NumberFormat(undefined, { notation: value >= 10000 ? 'compact' : 'standard' }).format(value);
}

function formatList(values: string[]) {
  return values.length ? values.join(', ') : '-';
}

function formatEvidenceProfile(profile: string) {
  const labels: Record<string, string> = {
    connectivity_smoke: 'connectivity smoke',
    prompt_smoke: 'prompt smoke',
    weak_prompt_suite: 'weak prompt suite',
    thin_prompt_suite: 'thin prompt suite',
    prompt_comparison: 'prompt comparison',
    code_smoke: 'code smoke',
    code_agent: 'code/agent',
    worker_harness: 'worker harness',
    empty: 'empty',
  };
  return labels[profile] ?? profile.replace(/_/g, ' ');
}

function knownCalibrationStatus(status: string) {
  return ['calibrated', 'pilot', 'reviewed', 'uncalibrated'].includes(status)
    ? status
    : 'uncalibrated';
}

function formatCalibrationStatus(status: string) {
  const labels: Record<string, string> = {
    calibrated: 'calibrated',
    pilot: 'pilot calibration',
    reviewed: 'reviewed',
    uncalibrated: 'uncalibrated',
    custom: 'custom calibration',
  };
  return labels[status] ?? status.replace(/_/g, ' ');
}

function packCalibrationSummary(pack: BenchmarkPack) {
  const parts = [`Calibration: ${formatCalibrationStatus(pack.calibrationStatus)}`];
  if (pack.calibrationSampleSize != null) {
    parts.push(`${pack.calibrationSampleSize} sample(s)`);
  }
  if (pack.calibrationBaselineModels.length) {
    parts.push(`baselines ${pack.calibrationBaselineModels.join(', ')}`);
  }
  if (pack.calibrationLastReviewed) {
    parts.push(`reviewed ${pack.calibrationLastReviewed}`);
  }
  if (pack.calibrationReviewScope && pack.calibrationReviewScope !== 'none') {
    parts.push(`scope ${pack.calibrationReviewScope.replace(/_/g, ' ')}`);
  }
  const qualityGates = pack.calibrationQualityGates ?? [];
  if (qualityGates.length) {
    parts.push(`${qualityGates.length} gate(s)`);
  }
  return parts.join('; ');
}

function taskEditableByPromptForm(task: BenchmarkPackTask) {
  const scoring = task.scoring ?? {};
  const structuredFields = ['json_field_equals', 'json_field_contains', 'json_field_array_exact', 'json_field_array_exact_ordered', 'json_field_object_keys_exact', 'json_field_number_close', 'json_field_number_bounds'];
  const primaryScorers = [
    typeof scoring.expect_exact === 'string' && scoring.expect_exact.trim().length > 0,
    stringArrayValue(scoring.expect_contains).length > 0,
    stringArrayValue(scoring.expect_regex).length > 0,
    ...structuredFields.map(field => objectKeysValue(scoring[field]).length > 0),
  ].filter(Boolean).length;
  return task.taskType === 'prompt'
    && primaryScorers <= 1
    && stringArrayValue(scoring.expect_not_contains).length === 0
    && !(Array.isArray(scoring.command) && scoring.command.length)
    && !scoring.parse;
}

function scoringEditorFromTask(task: BenchmarkPackTask) {
  const scoring = task.scoring ?? {};
  for (const field of ['json_field_equals', 'json_field_contains', 'json_field_array_exact', 'json_field_array_exact_ordered', 'json_field_object_keys_exact', 'json_field_number_close', 'json_field_number_bounds']) {
    if (objectKeysValue(scoring[field]).length) {
      return { method: field, expected: JSON.stringify(scoring[field], null, 2) };
    }
  }
  const exact = typeof scoring.expect_exact === 'string' ? scoring.expect_exact.trim() : '';
  if (exact) {
    return { method: 'exact', expected: exact };
  }
  const contains = stringArrayValue(scoring.expect_contains);
  if (contains.length) {
    return { method: 'contains', expected: contains.join('\n') };
  }
  const regex = stringArrayValue(scoring.expect_regex);
  if (regex.length) {
    return { method: 'regex', expected: regex.join('\n') };
  }
  if (scoring.expect_json === true) {
    return { method: 'json', expected: '' };
  }
  return { method: 'non_empty', expected: '' };
}

function scoringMethodRequiresExpected(method: string) {
  return method !== 'json' && method !== 'non_empty';
}

function scoringExpectedPlaceholder(method: string) {
  switch (method) {
    case 'exact':
      return 'OK';
    case 'contains':
      return 'local\ncloud';
    case 'regex':
      return '(?i)benchmark';
    case 'json_field_equals':
      return '{"status":"ok","allowed":true}';
    case 'json_field_contains':
      return '{"summary":["local","cloud"]}';
    case 'json_field_array_exact':
      return '{"evidence_ids":["A1","A2"]}';
    case 'json_field_array_exact_ordered':
      return '{"steps":["validate","run","export"]}';
    case 'json_field_object_keys_exact':
      return '{"$":["decision","reason","cost_usd"]}';
    case 'json_field_number_close':
      return '{"total_cost_usd":{"expected":0.05,"tolerance":0.001}}';
    case 'json_field_number_bounds':
      return '{"latency_ms":{"min":0,"max":5000}}';
    default:
      return '';
  }
}

function formatTaskScoringPreview(task: BenchmarkPackTask) {
  const scoring = task.scoring ?? {};
  const parts: string[] = [];
  const exact = typeof scoring.expect_exact === 'string' ? scoring.expect_exact.trim() : '';
  if (exact) {
    parts.push(`exact: ${shortPreview(exact)}`);
  }
  const contains = stringArrayValue(scoring.expect_contains);
  if (contains.length) {
    parts.push(`contains: ${contains.map(shortPreview).join(', ')}`);
  }
  const regex = stringArrayValue(scoring.expect_regex);
  if (regex.length) {
    parts.push(`regex: ${regex.map(shortPreview).join(', ')}`);
  }
  const notContains = stringArrayValue(scoring.expect_not_contains);
  if (notContains.length) {
    parts.push(`excludes: ${notContains.map(shortPreview).join(', ')}`);
  }
  if (scoring.expect_json === true) {
    parts.push('valid JSON');
  }
  for (const field of ['json_field_equals', 'json_field_contains', 'json_field_array_exact', 'json_field_array_exact_ordered', 'json_field_object_keys_exact', 'json_field_number_close', 'json_field_number_bounds']) {
    const keys = objectKeysValue(scoring[field]);
    if (keys.length) {
      parts.push(`${field.replace(/^json_field_/, '').replace(/_/g, ' ')}: ${keys.join(', ')}`);
    }
  }
  return parts.length ? parts.join('; ') : 'non-empty response';
}

function stringArrayValue(value: unknown) {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string' && item.trim().length > 0) : [];
}

function objectKeysValue(value: unknown) {
  return value && typeof value === 'object' && !Array.isArray(value) ? Object.keys(value) : [];
}

function shortPreview(value: string, limit = 52) {
  const normalized = value.replace(/\s+/g, ' ').trim();
  return normalized.length > limit ? `${normalized.slice(0, limit - 1)}...` : normalized;
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return '0 B';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size >= 10 || unit === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`;
}

function formatTokenSummary(result: RunResult) {
  if (result.total_tokens != null) {
    return `${Math.round(result.total_tokens)} total`;
  }
  if (result.prompt_tokens != null || result.completion_tokens != null) {
    return `${Math.round(result.prompt_tokens ?? 0)} in / ${Math.round(result.completion_tokens ?? 0)} out`;
  }
  return '-';
}

function slugify(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 64) || crypto.randomUUID();
}

function slugifyInput(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-|-$/g, '').slice(0, 64);
}

function Card({ title, value, note }: { title: string; value: string | number; note: string }) {
  return <div className="card"><div className="card-title">{title}</div><div className="card-value">{value}</div><div className="card-note">{note}</div></div>;
}
