import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { errorText } from "@/lib/net";
import { api } from "@/services/tauri";
import type { HealthReport } from "@/types";

export function ServicesPage() {
  const [reports, setReports] = useState<HealthReport[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    api.listHealth().then(setReports).catch((err: unknown) => setError(errorText(err)));
    const unlisten = listen<HealthReport[]>("health://status", (event) => setReports(event.payload));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  async function check() {
    setChecking(true);
    try {
      setReports(await api.checkHealth());
      setError(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setChecking(false);
    }
  }

  return (
    <div>
      <PageHeader
        title="Services"
        description="GET checks against each enabled mapping destination. The same client ignores HTTP_PROXY."
        actions={
          <Button type="button" variant="secondary" onClick={() => void check()} disabled={checking}>
            {checking ? "Checking" : "Check now"}
          </Button>
        }
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      <Card>
        <CardContent className="grid gap-3 py-4 text-sm">
          {reports.length === 0 ? <p className="text-muted-foreground">No destinations to check.</p> : null}
          {reports.map((report) => (
            <div key={report.destination} className="flex items-start justify-between gap-4 border-b border-border pb-3 last:border-b-0 last:pb-0">
              <div>
                <div className="font-mono text-xs">{report.destination}</div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {report.checkedAt} · {report.latencyMs} ms
                  {report.error ? ` · ${report.error}` : ""}
                </div>
              </div>
              <Badge variant={report.up ? "default" : "outline"}>{report.status ?? (report.up ? "up" : "down")}</Badge>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
