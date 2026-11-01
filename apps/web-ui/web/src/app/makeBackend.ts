import { isTauri } from "@tauri-apps/api/core";
import type { CairnClient } from "../client/types";
import type { CairnHost } from "../client/host";
import { MockClient } from "../client/mock";
import { MockHost } from "../client/host";
import { FIXTURE_NOTES } from "../client/fixtures";
import { TauriClient, TauriHost } from "../client/tauri";
import { DaemonClient, DaemonHost } from "../client/daemon";
import { loadDaemonConfig } from "../client/daemonConfig";

export interface Backend {
  client: CairnClient;
  host: CairnHost;
}

/** The single place the transport is chosen: Tauri in the app, a configured
 *  remote daemon in a browser, else the mock. */
export function makeBackend(): Backend {
  if (isTauri()) {
    return { client: new TauriClient(), host: new TauriHost() };
  }
  const daemon = loadDaemonConfig();
  if (daemon.url) {
    return {
      client: new DaemonClient(daemon),
      host: new DaemonHost(daemon.url),
    };
  }
  return { client: new MockClient(FIXTURE_NOTES), host: new MockHost() };
}
