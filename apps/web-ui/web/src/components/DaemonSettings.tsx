import { useState, type ChangeEvent } from "react";
import { Input } from "./ui/Input";
import { Button } from "./ui/Button";
import { SectionLabel } from "./ui/SectionLabel";
import {
  loadDaemonConfig,
  saveDaemonConfig,
  type DaemonConfig,
} from "../client/daemonConfig";

/** Configure the remote-daemon transport (browser/remote/multi-device). The
 *  transport is chosen once at the composition root, so a change only applies
 *  after a reload — deliberately not wired through the live store. */
export function DaemonSettings() {
  const [cfg, setCfg] = useState<DaemonConfig>(loadDaemonConfig);
  const [saved, setSaved] = useState(false);

  const patch = (p: Partial<DaemonConfig>) => {
    setCfg((c) => ({ ...c, ...p }));
    setSaved(false);
  };

  return (
    <div className="flex flex-col gap-2 text-sm text-text">
      <span className="mb-1">
        <SectionLabel>Remote daemon</SectionLabel>
      </span>
      <label className="flex flex-col gap-1 text-muted">
        Daemon URL
        <Input
          aria-label="Daemon URL"
          placeholder="http://localhost:7777"
          value={cfg.url}
          onChange={(e: ChangeEvent<HTMLInputElement>) =>
            patch({ url: e.target.value })
          }
        />
      </label>
      <label className="flex flex-col gap-1 text-muted">
        Bearer token
        <Input
          aria-label="Bearer token"
          type="password"
          placeholder="from <cairn>/.cairn/token"
          value={cfg.token}
          onChange={(e: ChangeEvent<HTMLInputElement>) =>
            patch({ token: e.target.value })
          }
        />
      </label>
      <div className="flex items-center gap-3">
        <Button
          onClick={() => {
            saveDaemonConfig(cfg);
            setSaved(true);
          }}
        >
          Save
        </Button>
        {saved && <span className="text-accent">Saved</span>}
      </div>
      <p className="text-xs text-muted">
        Connecting to a daemon takes effect after a reload. Leave the URL blank
        to use the built-in mock.
      </p>
    </div>
  );
}
