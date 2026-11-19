import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { errorText } from "@/lib/net";
import { api } from "@/services/tauri";
import type { CaStatus, ProxyStatus, Rule, SystemProxyStatus, TrafficEvent } from "@/types";

export function DashboardPage() {
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [systemProxy, setSystemProxy] = useState<SystemProxyStatus | null>(null);
  const [ca, setCa] = useState<CaStatus | null>(null);
  const [recent, setRecent] = useState<TrafficEvent[]>([]);
  const [rules, setRules] = useState<Rule[]>([]);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    setStatus(await api.proxyStatus());
    setSystemProxy(await api.systemProxyStatus());
    setCa(await api.caStatus());
  }

  useEffect(() => {
    refresh().catch((err: unknown) => setError(errorText(err)));
    api.listTraffic().then((events) => setRecent(events.slice(0, 8))).catch((err: unknown) => setError(errorText(err)));
    api.listRules().then(setRules).catch((err: unknown) => setError(errorText(err)));
    const unlisten = listen<TrafficEvent>("traffic://event", () => {
      void api.listTraffic().then((events) => setRecent(events.slice(0, 8)));
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  async function trustCertificate() {
    try {
      setCa(await api.trustCa());
      setError(null);
    } catch (err) {
      setCa(await api.caStatus().catch(() => ca));
      setError(errorText(err));
    }
  }

  async function toggleBrowser() {
    try {
      const next = systemProxy?.enabled ? await api.disableSystemProxy() : await api.enableSystemProxy();
      setSystemProxy(next);
      setStatus(await api.proxyStatus());
      setError(null);
    } catch (err) {
      setSystemProxy(await api.systemProxyStatus().catch(() => systemProxy));
      setError(errorText(err));
    }
  }

  async function toggle() {
    try {
      const next = status?.running ? await api.stopProxy() : await api.startProxy();
      setStatus(next);
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <div>
      <PageHeader
        title="Dashboard"
        description="The listener accepts absolute-form HTTP on loopback and sends mapped hosts to their local destination."
        actions={
          <Button type="button" onClick={() => void toggle()} variant={status?.running ? "secondary" : "default"}>
            {status?.running ? "Stop listener" : "Start listener"}
          </Button>
        }
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      <div className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader>
            <CardTitle>Listener</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-2 text-sm">
            <div className="flex items-center gap-2">
              <Badge variant={status?.running ? "default" : "outline"}>{status?.running ? "Running" : "Stopped"}</Badge>
              <span className="font-mono text-xs text-muted-foreground">
                {status?.address ?? `${status?.bindHost ?? "127.0.0.1"}:${status?.port ?? 8787}`}
              </span>
            </div>
            {status && !status.loopbackBind ? (
              <p className="text-destructive">The configured bind is not loopback.</p>
            ) : (
              <p className="text-muted-foreground">Bind changes apply the next time the listener starts.</p>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Browser</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-2 text-sm">
            <div className="flex items-center gap-2">
              <Badge variant={systemProxy?.enabled ? "default" : "outline"}>{systemProxy?.enabled ? "On" : "Off"}</Badge>
              <Button
                type="button"
                size="sm"
                variant={systemProxy?.enabled ? "secondary" : "default"}
                onClick={() => void toggleBrowser()}
              >
                {systemProxy?.enabled ? "Stop using in browser" : "Use in browser"}
              </Button>
            </div>
            <p className="text-muted-foreground">
              Points HTTP and HTTPS at {systemProxy?.address ?? "127.0.0.1:8787"} and bypasses localhost, 127.0.0.1, and ::1.
              Firefox follows this only when it is set to the system proxy.
            </p>
            {systemProxy?.error ? (
              <p className="text-destructive">
                The operating system did not change the proxy. Set it manually: {systemProxy.manualHint} {systemProxy.error}
              </p>
            ) : null}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Certificate authority</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-2 text-sm">
            <div className="flex items-center gap-2">
              <div className="font-medium text-foreground">{ca?.subject || "Local development CA"}</div>
              <Badge variant={ca?.trusted ? "default" : "outline"}>{ca?.trusted ? "Trusted" : "Pending"}</Badge>
            </div>
            <p className="break-all font-mono text-[11px] text-muted-foreground">{ca?.fingerprint || "Fingerprint appears after the CA is created."}</p>
            <Button type="button" size="sm" variant="secondary" onClick={() => void trustCertificate()}>
              Trust certificate authority
            </Button>
            {ca?.trustCommandSucceeded ? <p>The operating system accepted the trust command in this session.</p> : null}
            {ca?.trustError ? <p className="text-destructive">The operating system did not trust the CA. {ca.trustError}</p> : null}
            {!ca?.available && ca ? <p className="text-destructive">The local CA is not available.</p> : null}
            <p className="text-muted-foreground">
              {status?.httpsInterception ? "HTTPS interception is enabled." : "HTTPS interception is off."} Only mapped hosts are terminated.
              Certificate pinning and wss:// are not rewritten. Upstream certificates stay verified.
            </p>
          </CardContent>
        </Card>
      </div>
      <Card className="mt-4">
        <CardHeader>
          <CardTitle>Active mappings</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-2 text-sm">
          {rules.filter((rule) => rule.enabled).length === 0 ? (
            <p className="text-muted-foreground">No enabled mappings on the active profile.</p>
          ) : null}
          {rules
            .filter((rule) => rule.enabled)
            .map((rule) => (
              <div key={rule.id} className="flex items-center gap-3 font-mono text-xs">
                <span className="w-40 truncate text-foreground">{rule.name}</span>
                <span className="min-w-0 flex-1 truncate text-muted-foreground">
                  {rule.sourceHost}
                  {rule.sourcePort ? `:${rule.sourcePort}` : ""} → {rule.destination}
                </span>
              </div>
            ))}
        </CardContent>
      </Card>
      <Card className="mt-4">
        <CardHeader>
          <CardTitle>Recent traffic</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-2 text-sm">
          {recent.length === 0 ? <p className="text-muted-foreground">No requests yet.</p> : null}
          {recent.map((event) => (
            <div key={event.id} className="flex items-center gap-3 font-mono text-xs">
              <span className="w-12">{event.method}</span>
              <span className="w-10">{event.status ?? "—"}</span>
              <span className="min-w-0 flex-1 truncate">{event.url}</span>
              {event.mapped ? <Badge>mapped</Badge> : null}
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
