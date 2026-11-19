import { useEffect, useState, type ReactNode } from "react";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { errorText, isLoopbackBind } from "@/lib/net";
import { api } from "@/services/tauri";
import type { CaStatus, Settings } from "@/types";

export function SettingsPage() {
  const [form, setForm] = useState<Settings | null>(null);
  const [ca, setCa] = useState<CaStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api
      .getSettings()
      .then((settings) => {
        setForm(settings);
        setError(null);
      })
      .catch((err: unknown) => setError(errorText(err)));
    api.caStatus().then(setCa).catch((err: unknown) => setError(errorText(err)));
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

  async function save() {
    if (!form) return;
    setSaved(false);
    try {
      const next = await api.updateSettings({
        ...form,
        port: Number(form.port),
        connectTimeoutMs: Number(form.connectTimeoutMs),
        requestTimeoutMs: Number(form.requestTimeoutMs),
        bodyLimitBytes: Number(form.bodyLimitBytes),
      });
      setForm(next);
      setError(null);
      setSaved(true);
    } catch (err) {
      setError(errorText(err));
    }
  }

  if (!form) {
    return (
      <div>
        <PageHeader title="Settings" description="Bind address, port, timeouts, privacy, and HTTPS interception." />
        {error ? <p className="text-sm text-destructive">{error}</p> : <p className="text-sm text-muted-foreground">Loading settings.</p>}
      </div>
    );
  }

  const offLoopback = !isLoopbackBind(form.bindHost);

  return (
    <div>
      <PageHeader
        title="Settings"
        description="Bind address, port, timeouts, privacy, and HTTPS interception. Bind and port apply the next time the listener starts."
        actions={
          <Button onClick={() => void save()} type="button">
            Save
          </Button>
        }
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      {saved ? <p className="mb-4 text-sm text-primary">Saved.</p> : null}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Listener</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-3">
            <Field label="Bind address">
              <Input value={form.bindHost} onChange={(event) => setForm({ ...form, bindHost: event.target.value })} />
            </Field>
            <Field label="Port">
              <Input
                type="number"
                value={form.port}
                onChange={(event) => setForm({ ...form, port: Number(event.target.value) })}
              />
            </Field>
            <Field label="Connect timeout (ms)">
              <Input
                type="number"
                value={form.connectTimeoutMs}
                onChange={(event) => setForm({ ...form, connectTimeoutMs: Number(event.target.value) })}
              />
            </Field>
            <Field label="Request timeout (ms)">
              <Input
                type="number"
                value={form.requestTimeoutMs}
                onChange={(event) => setForm({ ...form, requestTimeoutMs: Number(event.target.value) })}
              />
            </Field>
            {offLoopback ? (
              <p className="text-sm text-destructive">
                This bind is not loopback. The listener will accept connections from other machines. Keep it on 127.0.0.1 unless you mean to expose it.
              </p>
            ) : (
              <p className="text-sm text-muted-foreground">Default is 127.0.0.1:8787. The listener does not need administrator rights.</p>
            )}
            <Toggle
              label="Start listener when the app opens"
              checked={form.autostartProxy}
              onCheckedChange={(autostartProxy) => setForm({ ...form, autostartProxy })}
            />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Privacy and HTTPS</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-3">
            <Toggle
              label="Redact Authorization, Cookie, and proxy credentials in the inspector"
              checked={form.redactSensitive}
              onCheckedChange={(redactSensitive) => setForm({ ...form, redactSensitive })}
            />
            <Toggle
              label="Keep a larger body capture in the inspector"
              checked={form.captureBody}
              onCheckedChange={(captureBody) => setForm({ ...form, captureBody })}
            />
            <Field label="Body capture limit (bytes, max 1 MiB)">
              <Input
                type="number"
                value={form.bodyLimitBytes}
                onChange={(event) => setForm({ ...form, bodyLimitBytes: Number(event.target.value) })}
              />
            </Field>
            <div className="rounded-md border border-border px-3 py-2 text-sm">
              <div className="flex items-center gap-2">
                <span className="font-medium">{ca?.subject || "Local development CA"}</span>
                <Badge variant={ca?.trusted ? "default" : "outline"}>{ca?.trusted ? "Trusted" : "Pending"}</Badge>
              </div>
              <p className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                {ca?.fingerprint || "Fingerprint appears after the CA is created."}
              </p>
              <Button className="mt-2" type="button" size="sm" variant="secondary" onClick={() => void trustCertificate()}>
                Trust certificate authority
              </Button>
              <p className="mt-2 text-muted-foreground">
                Pending means this CA is not trusted in the login keychain yet. Trust is explicit and may ask for your password. The app does not install it on its own.
              </p>
            </div>
            <Toggle
              label="Intercept HTTPS for mapped hosts"
              checked={form.httpsInterception}
              onCheckedChange={(httpsInterception) => setForm({ ...form, httpsInterception })}
            />
            <p className="text-sm text-muted-foreground">
              Interception terminates TLS only when the host has an enabled mapping. Other connections stay end-to-end tunnels. Trust the local CA before enabling this. Sites that pin certificates will reject the leaf. WebSocket upgrades are not intercepted.
            </p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-1.5">
      <Label>{label}</Label>
      {children}
    </label>
  );
}

function Toggle({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-md border border-border px-3 py-2">
      <span className="text-sm">{label}</span>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}
