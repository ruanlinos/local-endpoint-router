import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { errorText } from "@/lib/net";
import { api } from "@/services/tauri";
import type { TrafficEvent } from "@/types";

export function TrafficPage() {
  const [events, setEvents] = useState<TrafficEvent[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [mappedOnly, setMappedOnly] = useState(false);
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listTraffic().then(setEvents).catch((err: unknown) => setError(errorText(err)));
    const unlisten = listen<TrafficEvent>("traffic://event", (event) => {
      setEvents((current) => upsert(current, event.payload));
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return events.filter((event) => {
      if (mappedOnly && !event.mapped) return false;
      if (errorsOnly && !isError(event)) return false;
      if (!needle) return true;
      return `${event.method} ${event.url} ${event.host} ${event.status ?? ""} ${event.ruleName ?? ""}`
        .toLowerCase()
        .includes(needle);
    });
  }, [events, query, mappedOnly, errorsOnly]);

  const current = visible.find((event) => event.id === selected) ?? visible[0] ?? null;

  return (
    <div>
      <PageHeader
        title="Traffic"
        description="Live requests from the listener. Authorization, Cookie, Proxy-Authorization, and Set-Cookie values are redacted in this view."
        actions={
          <Button
            type="button"
            variant="secondary"
            onClick={() =>
              void api
                .clearTraffic()
                .then(() => {
                  setEvents([]);
                  setSelected(null);
                })
                .catch((err: unknown) => setError(errorText(err)))
            }
          >
            Clear
          </Button>
        }
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <Input className="max-w-sm" value={query} placeholder="Filter host, method, status" onChange={(event) => setQuery(event.target.value)} />
        <FilterToggle label="Mapped" checked={mappedOnly} onChange={setMappedOnly} />
        <FilterToggle label="Errors" checked={errorsOnly} onChange={setErrorsOnly} />
      </div>
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
        <Card>
          <CardContent className="max-h-[32rem] overflow-auto p-0">
            {visible.length === 0 ? (
              <p className="px-4 py-8 text-sm text-muted-foreground">No requests yet. Traffic appears when the listener handles one.</p>
            ) : (
              visible.map((event) => (
                <button
                  key={event.id}
                  type="button"
                  onClick={() => setSelected(event.id)}
                  className={`flex w-full items-center gap-3 border-b border-border px-4 py-2 text-left text-sm last:border-b-0 ${
                    current?.id === event.id ? "bg-accent" : "hover:bg-accent/50"
                  }`}
                >
                  <span className="w-14 font-mono text-xs">{event.method}</span>
                  <span className="w-12 font-mono text-xs">{event.status ?? "—"}</span>
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">{event.url}</span>
                  {event.mapped ? <Badge>mapped</Badge> : <Badge variant="outline">{event.kind}</Badge>}
                </button>
              ))
            )}
          </CardContent>
        </Card>
        <Card>
          <CardContent className="max-h-[32rem] overflow-auto py-4 text-sm">
            {current ? <Inspector event={current} /> : <p className="text-muted-foreground">Select a request.</p>}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function Inspector({ event }: { event: TrafficEvent }) {
  return (
    <div className="grid gap-3">
      <div>
        <div className="font-medium">{event.method} {event.url}</div>
        <div className="mt-1 text-xs text-muted-foreground">
          {event.timestamp} · {event.durationMs} ms · {event.ruleName ?? "no rule"}
        </div>
        {event.error ? <p className="mt-2 text-destructive">{event.error}</p> : null}
      </div>
      <HeaderBlock title="Request headers" headers={event.requestHeaders} />
      <BodyBlock title="Request body" body={event.requestBodyPreview} truncated={event.requestTruncated} />
      <HeaderBlock title="Response headers" headers={event.responseHeaders} />
      <BodyBlock title="Response body" body={event.responseBodyPreview} truncated={event.responseTruncated} />
    </div>
  );
}

function HeaderBlock({ title, headers }: { title: string; headers: [string, string][] }) {
  return (
    <div>
      <div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">{title}</div>
      <pre className="overflow-auto rounded-md bg-background p-2 font-mono text-[11px] leading-relaxed">
        {headers.length === 0 ? "—" : headers.map(([name, value]) => `${name}: ${value}`).join("\n")}
      </pre>
    </div>
  );
}

function BodyBlock({ title, body, truncated }: { title: string; body: string; truncated: boolean }) {
  return (
    <div>
      <div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">
        {title}
        {truncated ? " · truncated" : ""}
      </div>
      <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded-md bg-background p-2 font-mono text-[11px]">
        {body || "—"}
      </pre>
    </div>
  );
}

function FilterToggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className={`rounded-md border px-2 py-1 text-xs ${checked ? "border-primary text-primary" : "border-border text-muted-foreground"}`}
    >
      {label}
    </button>
  );
}

function isError(event: TrafficEvent) {
  return Boolean(event.error) || (event.status != null && event.status >= 400);
}

function upsert(current: TrafficEvent[], event: TrafficEvent) {
  const index = current.findIndex((item) => item.id === event.id);
  if (index === -1) return [event, ...current].slice(0, 500);
  const next = current.slice();
  next[index] = event;
  return next;
}
