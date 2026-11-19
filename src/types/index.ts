export interface Settings {
  bindHost: string;
  port: number;
  connectTimeoutMs: number;
  requestTimeoutMs: number;
  redactSensitive: boolean;
  captureBody: boolean;
  bodyLimitBytes: number;
  httpsInterception: boolean;
  autostartProxy: boolean;
  activeProfileId: string;
}

export interface TrafficEvent {
  id: number;
  timestamp: string;
  method: string;
  url: string;
  host: string;
  status: number | null;
  mapped: boolean;
  ruleId: string | null;
  ruleName: string | null;
  error: string | null;
  durationMs: number;
  kind: string;
  requestHeaders: [string, string][];
  responseHeaders: [string, string][];
  requestBodyPreview: string;
  responseBodyPreview: string;
  requestTruncated: boolean;
  responseTruncated: boolean;
}

export interface HealthReport {
  destination: string;
  up: boolean;
  status: number | null;
  latencyMs: number;
  error: string | null;
  checkedAt: string;
}

export interface CaStatus {
  available: boolean;
  subject: string;
  fingerprint: string;
  path: string;
  trustError: string | null;
  trustCommandSucceeded: boolean;
  trusted: boolean;
}

export interface SystemProxyStatus {
  enabled: boolean;
  address: string;
  error: string | null;
  manualHint: string;
}

export interface ProxyStatus {
  running: boolean;
  address: string | null;
  bindHost: string;
  port: number;
  loopbackBind: boolean;
  httpsInterception: boolean;
}

export interface Profile {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
}

export interface RuleInput {
  name: string;
  enabled: boolean;
  priority: number;
  sourceHost: string;
  sourcePort: number | null;
  methods: string[];
  pathPrefix: string | null;
  rewriteFrom: string | null;
  rewriteTo: string | null;
  destination: string;
}

export interface Rule extends RuleInput {
  id: string;
  profileId: string;
  createdAt: string;
  updatedAt: string;
}

export interface ProfileDocument {
  version: number;
  name: string;
  rules: RuleInput[];
}
