import { useEffect, useState } from "react";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { errorText } from "@/lib/net";
import { api } from "@/services/tauri";
import type { Profile, ProfileDocument, Settings } from "@/types";

export function ProfilesPage() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [name, setName] = useState("");
  const [exportJson, setExportJson] = useState("");
  const [importJson, setImportJson] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function reload() {
    const [nextProfiles, nextSettings] = await Promise.all([api.listProfiles(), api.getSettings()]);
    setProfiles(nextProfiles);
    setSettings(nextSettings);
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
        title="Profiles"
        description="A profile is a named set of mappings. The active profile is the one the listener matches. Import and export use JSON."
      />
      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}
      <div className="mb-4 flex gap-2">
        <Input
          value={name}
          placeholder="New profile name"
          onChange={(event) => setName(event.target.value)}
        />
        <Button
          type="button"
          onClick={() =>
            void run(async () => {
              await api.createProfile(name);
              setName("");
              await reload();
            })
          }
        >
          Create
        </Button>
      </div>
      <div className="grid gap-3">
        {profiles.map((profile) => {
          const active = settings?.activeProfileId === profile.id;
          return (
            <Card key={profile.id}>
              <CardContent className="flex flex-wrap items-center gap-2 py-4">
                <div className="min-w-40 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">{profile.name}</span>
                    {active ? <Badge>Active</Badge> : null}
                  </div>
                  <div className="mt-1 font-mono text-[11px] text-muted-foreground">{profile.id}</div>
                </div>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  disabled={active}
                  onClick={() =>
                    void run(async () => {
                      setSettings(await api.setActiveProfile(profile.id));
                    })
                  }
                >
                  Use
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    void run(async () => {
                      const next = window.prompt("Profile name", profile.name);
                      if (!next) return;
                      await api.renameProfile(profile.id, next);
                      await reload();
                    })
                  }
                >
                  Rename
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    void run(async () => {
                      const document = await api.exportProfile(profile.id);
                      setExportJson(JSON.stringify(document, null, 2));
                    })
                  }
                >
                  Export
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  onClick={() =>
                    void run(async () => {
                      const next = await api.deleteProfile(profile.id);
                      setSettings(next);
                      await reload();
                    })
                  }
                >
                  Delete
                </Button>
              </CardContent>
            </Card>
          );
        })}
      </div>
      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <div className="grid gap-2">
          <div className="text-sm font-medium">Export</div>
          <Textarea readOnly value={exportJson} placeholder="Choose Export on a profile." />
          <Button
            type="button"
            variant="secondary"
            disabled={!exportJson}
            onClick={() => downloadJson(exportJson)}
          >
            Download JSON
          </Button>
        </div>
        <div className="grid gap-2">
          <div className="text-sm font-medium">Import</div>
          <Textarea
            value={importJson}
            placeholder='{"version":1,"name":"Work","rules":[]}'
            onChange={(event) => setImportJson(event.target.value)}
          />
          <Button
            type="button"
            onClick={() =>
              void run(async () => {
                const document = JSON.parse(importJson) as ProfileDocument;
                await api.importProfile(document);
                setImportJson("");
                await reload();
              })
            }
          >
            Import profile
          </Button>
        </div>
      </div>
    </div>
  );
}

function downloadJson(json: string) {
  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "profile.json";
  link.click();
  URL.revokeObjectURL(url);
}
