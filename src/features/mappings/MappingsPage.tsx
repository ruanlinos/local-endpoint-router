import { useEffect, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { errorText } from "@/lib/net";
import { api } from "@/services/tauri";
import type { Rule, RuleInput } from "@/types";

interface Draft {
  name: string;
  enabled: boolean;
  priority: string;
  sourceHost: string;
  sourcePort: string;
  methods: string;
  pathPrefix: string;
  rewriteFrom: string;
  rewriteTo: string;
  destination: string;
}

const EMPTY: Draft = {
  name: "",
  enabled: true,
  priority: "100",
  sourceHost: "",
  sourcePort: "",
  methods: "",
  pathPrefix: "",
  rewriteFrom: "",
  rewriteTo: "",
  destination: "http://127.0.0.1:",
};

function toInput(draft: Draft): RuleInput {
  const methods = draft.methods
    .split(",")
    .map((method) => method.trim())
    .filter(Boolean);
  return {
    name: draft.name,
    enabled: draft.enabled,
    priority: Number(draft.priority),
    sourceHost: draft.sourceHost,
    sourcePort: draft.sourcePort.trim() ? Number(draft.sourcePort) : null,
    methods,
    pathPrefix: draft.pathPrefix.trim() || null,
    rewriteFrom: draft.rewriteFrom.trim() || null,
    rewriteTo: draft.rewriteTo.trim() || null,
    destination: draft.destination,
  };
}

function fromRule(rule: Rule): Draft {
  return {
    name: rule.name,
    enabled: rule.enabled,
    priority: String(rule.priority),
    sourceHost: rule.sourceHost,
    sourcePort: rule.sourcePort == null ? "" : String(rule.sourcePort),
    methods: rule.methods.join(", "),
    pathPrefix: rule.pathPrefix ?? "",
    rewriteFrom: rule.rewriteFrom ?? "",
    rewriteTo: rule.rewriteTo ?? "",
    destination: rule.destination,
  };
}

export function MappingsPage() {
  const [rules, setRules] = useState<Rule[]>([]);
  const [draft, setDraft] = useState<Draft>(EMPTY);
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    setRules(await api.listRules());
  }

  useEffect(() => {
    reload().catch((err: unknown) => setError(errorText(err)));
  }, []);

  async function run(action: () => Promise<void>) {
    try {
      await action();
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <div>
      <PageHeader
        title="Mappings"
        description="A matched host is sent to the local destination with its original path and query. Enable and disable updates the live snapshot. The listener does not restart."
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      <Card className="mb-4">
        <CardContent className="grid gap-3 py-4 md:grid-cols-2">
          <Field label="Name" value={draft.name} onChange={(name) => setDraft({ ...draft, name })} />
          <Field
            label="Source host"
            value={draft.sourceHost}
            placeholder="payments.api.company.com or *.company.com"
            onChange={(sourceHost) => setDraft({ ...draft, sourceHost })}
          />
          <Field
            label="Source port"
            value={draft.sourcePort}
            placeholder="any"
            onChange={(sourcePort) => setDraft({ ...draft, sourcePort })}
          />
          <Field
            label="Priority (lower wins)"
            value={draft.priority}
            onChange={(priority) => setDraft({ ...draft, priority })}
          />
          <Field
            label="Methods"
            value={draft.methods}
            placeholder="GET, POST — empty matches all"
            onChange={(methods) => setDraft({ ...draft, methods })}
          />
          <Field
            label="Path prefix"
            value={draft.pathPrefix}
            placeholder="/api"
            onChange={(pathPrefix) => setDraft({ ...draft, pathPrefix })}
          />
          <Field
            label="Rewrite from"
            value={draft.rewriteFrom}
            placeholder="/api"
            onChange={(rewriteFrom) => setDraft({ ...draft, rewriteFrom })}
          />
          <Field
            label="Rewrite to"
            value={draft.rewriteTo}
            placeholder="/v2"
            onChange={(rewriteTo) => setDraft({ ...draft, rewriteTo })}
          />
          <Field
            label="Destination"
            value={draft.destination}
            placeholder="http://127.0.0.1:5002"
            onChange={(destination) => setDraft({ ...draft, destination })}
          />
          <div className="flex items-end justify-between gap-3">
            <div className="flex items-center gap-2 text-sm">
              <Switch checked={draft.enabled} onCheckedChange={(enabled) => setDraft({ ...draft, enabled })} />
              Enabled
            </div>
            <div className="flex gap-2">
              {editing ? (
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => {
                    setEditing(null);
                    setDraft(EMPTY);
                  }}
                >
                  Cancel
                </Button>
              ) : null}
              <Button
                type="button"
                onClick={() =>
                  void run(async () => {
                    const input = toInput(draft);
                    if (editing) await api.updateRule(editing, input);
                    else await api.createRule(input);
                    setEditing(null);
                    setDraft(EMPTY);
                    await reload();
                  })
                }
              >
                {editing ? "Save mapping" : "Add mapping"}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
      <div className="grid gap-3">
        {rules.length === 0 ? (
          <Card>
            <CardContent className="py-8 text-sm text-muted-foreground">No mappings on the active profile.</CardContent>
          </Card>
        ) : null}
        {rules.map((rule) => (
          <Card key={rule.id}>
            <CardContent className="flex flex-wrap items-center gap-3 py-4">
              <Switch
                checked={rule.enabled}
                onCheckedChange={(enabled) =>
                  void run(async () => {
                    await api.setRuleEnabled(rule.id, enabled);
                    await reload();
                  })
                }
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium">{rule.name}</span>
                  <Badge variant="outline">priority {rule.priority}</Badge>
                  {!rule.enabled ? <Badge variant="secondary">disabled</Badge> : null}
                </div>
                <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
                  {rule.sourceHost}
                  {rule.sourcePort ? `:${rule.sourcePort}` : ""}
                  {rule.pathPrefix ? ` ${rule.pathPrefix}` : ""} → {rule.destination}
                </div>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => {
                  setEditing(rule.id);
                  setDraft(fromRule(rule));
                }}
              >
                Edit
              </Button>
              <Button
                type="button"
                size="sm"
                variant="destructive"
                onClick={() =>
                  void run(async () => {
                    await api.deleteRule(rule.id);
                    if (editing === rule.id) {
                      setEditing(null);
                      setDraft(EMPTY);
                    }
                    await reload();
                  })
                }
              >
                Delete
              </Button>
            </CardContent>
          </Card>
        ))}
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  placeholder,
  onChange,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-1.5">
      <Label>{label}</Label>
      <Input value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}
