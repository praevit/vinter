import { useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { TriangleAlert, Trash2 } from "lucide-react";
import { useAppInfo } from "../api/app";
import {
  commands,
  type D3dMetalSource,
  type FlagState,
  type Settings_Serialize as SettingsData,
} from "../api/bindings";
import { errorMessage } from "../api/errors";
import { useExperimentalFlags, useSetExperimental } from "../api/experimental";
import { formatBytes, useAvailableRuntimes, useInstalledRuntimes } from "../api/runtimes";
import { useSaveSettings, useSettings } from "../api/settings";
import { BottleOptionsFields } from "../components/BottleOptionsFields";
import { Dialog } from "../components/Dialog";
import { ExperimentalBadge, RiskBadge } from "../components/ExperimentalBadge";
import { Select } from "../components/Select";
import { Button, ErrorText, subtle } from "../components/ui";

export function Settings() {
  const { data: view } = useSettings();
  const { data: app } = useAppInfo();
  const { data: runtimes } = useInstalledRuntimes();
  const { data: available } = useAvailableRuntimes();
  const save = useSaveSettings();
  const queryClient = useQueryClient();
  const [loginError, setLoginError] = useState<string>();

  // All hooks above this line: React needs the same ones on every render.
  if (!view) return null;
  const { launch_at_login, ...settings } = view;
  const set = (patch: Partial<SettingsData>) => save.mutate({ ...settings, ...patch });
  const mac = app?.os === "macos";
  const d3dmetalRuntime = [...(runtimes ?? []), ...(available ?? [])].some((r) =>
    r.graphics.includes("d3dmetal"),
  );

  const setLogin = async (enabled: boolean) => {
    setLoginError(undefined);
    try {
      await commands.settingsSetLaunchAtLogin(enabled);
    } catch (e) {
      setLoginError(errorMessage(e));
    }
    await queryClient.invalidateQueries({ queryKey: ["settings"] });
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-8 py-10">
      <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>

      <Group title="General">
        <Toggle
          title="Start Vinter at login"
          detail={`Vinter starts in the ${mac ? "menu bar" : "system tray"}, without its window.`}
          checked={launch_at_login}
          onChange={setLogin}
        />
        <Toggle
          title={mac ? "Show in the menu bar" : "Show in the system tray"}
          detail="Start your apps from the icon. Closing the window keeps Vinter running there."
          checked={settings.menu_bar_icon}
          onChange={(menu_bar_icon) => set({ menu_bar_icon })}
        />
        <Toggle
          title="Notifications"
          detail="When an app is ready, a runtime is installed, or a program quits right after starting."
          checked={settings.notifications}
          onChange={(notifications) => set({ notifications })}
        />
        <Toggle
          title="Check for updates automatically"
          detail="Vinter looks for a newer version of itself and of your runtimes when it starts and every few hours, and tells you. It never installs anything by itself."
          checked={settings.check_for_updates}
          onChange={(check_for_updates) => set({ check_for_updates })}
        />
      </Group>
      {loginError && <ErrorText>{loginError}</ErrorText>}

      <Group title="New bottles">
        <div className="flex flex-col gap-1.5 px-5 py-3.5">
          <span className="text-sm font-medium">Runtime</span>
          <Select
            value={settings.default_runtime ?? ""}
            onChange={(id) => set({ default_runtime: id || null })}
            options={[
              { value: "", label: "The newest installed one" },
              ...(runtimes?.map((r) => ({ value: r.id, label: r.name })) ?? []),
            ]}
          />
          <span className={subtle}>Apps set up with one click pick the runtime their recipe needs.</span>
        </div>
        <div className="px-5 py-3.5">
          <BottleOptionsFields
            value={settings.new_bottle_options}
            os={app?.os}
            onChange={(new_bottle_options) => set({ new_bottle_options })}
          />
        </div>
      </Group>
      {save.error && <ErrorText>{errorMessage(save.error)}</ErrorText>}

      {/* Only once a runtime can use it (none can yet; see the D3DMetal backend). */}
      {mac && d3dmetalRuntime && <D3dMetalGroup />}

      <StorageGroup />

      <ExperimentalGroup />
    </div>
  );
}

/** Importing Apple's D3DMetal from the user's own Game Porting Toolkit. */
function D3dMetalGroup() {
  const queryClient = useQueryClient();
  const { data: imported } = useQuery({ queryKey: ["d3dmetal"], queryFn: () => commands.d3dmetalStatus() });
  const [source, setSource] = useState<D3dMetalSource | null>(null);
  const [agreed, setAgreed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const refresh = () => queryClient.invalidateQueries();
  const choose = async () => {
    setError(undefined);
    try {
      const picked = await commands.d3dmetalChoose();
      if (picked) {
        setSource(picked);
        setAgreed(false);
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  const importIt = async () => {
    if (!source) return;
    setBusy(true);
    setError(undefined);
    try {
      await commands.d3dmetalImport(source.path);
      setSource(null);
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    setError(undefined);
    try {
      await commands.d3dmetalRemove();
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Group title="D3DMetal">
      <div className="flex items-center gap-4 px-5 py-3.5">
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="text-sm font-medium">
            {imported ? `Apple D3DMetal ${imported.version}` : "Apple's Direct3D 11 and 12 over Metal"}
          </span>
          <span className={subtle}>
            {imported
              ? "Bottles on a Vinter Wine made for D3DMetal can use it."
              : "Vinter cannot include it. Download the Game Porting Toolkit from Apple (free Apple developer sign-in), then import the .dmg here."}
          </span>
        </span>
        {imported ? (
          <Button onClick={remove}>
            <Trash2 className="size-4" /> Remove
          </Button>
        ) : (
          <Button onClick={choose}>Import…</Button>
        )}
      </div>
      {!imported && (
        <div className="px-5 py-3">
          <button
            type="button"
            className={`${subtle} underline`}
            onClick={() => commands.appOpenLink("game_porting_toolkit")}
          >
            Get the Game Porting Toolkit from Apple
          </button>
        </div>
      )}
      {error && (
        <div className="px-5 py-3">
          <ErrorText>{error}</ErrorText>
        </div>
      )}
      {source && (
        <Dialog onClose={busy ? undefined : () => setSource(null)}>
          <div className="flex flex-col gap-4">
            <h2 className="text-lg font-semibold">Apple's license for D3DMetal {source.version}</h2>
            <pre className="max-h-72 overflow-y-auto rounded-lg bg-frost-100 p-3 text-xs whitespace-pre-wrap select-text dark:bg-slate-deep-950">
              {source.license.trim() || "This copy has no license file."}
            </pre>
            <label className="flex items-start gap-2 text-sm">
              <input
                type="checkbox"
                className="mt-0.5 accent-wine-500"
                checked={agreed}
                onChange={(e) => setAgreed(e.target.checked)}
              />
              I agree to Apple's license. D3DMetal stays on this Mac; Vinter does not share it.
            </label>
            <div className="flex justify-end gap-2">
              <Button onClick={() => setSource(null)} disabled={busy}>
                Cancel
              </Button>
              <Button variant="primary" disabled={!agreed || busy} onClick={importIt}>
                {busy ? "Importing…" : "Import"}
              </Button>
            </div>
          </div>
        </Dialog>
      )}
    </Group>
  );
}

/** Experimental features, each off until the user turns it on. */
function ExperimentalGroup() {
  const { data: flags } = useExperimentalFlags();
  const setFlag = useSetExperimental();
  const [confirming, setConfirming] = useState<FlagState>();

  const toggle = (f: FlagState, enabled: boolean) => {
    if (enabled && f.risk === "high") setConfirming(f);
    else setFlag.mutate({ flag: f.flag, enabled });
  };

  return (
    <section id="experimental" className="flex flex-col gap-2">
      <h2 className="flex items-center gap-2 px-1 text-sm font-medium">
        Experimental <ExperimentalBadge />
      </h2>
      <div className="flex items-start gap-2.5 rounded-xl border border-amber-300/60 bg-amber-50 p-3.5 text-sm dark:border-amber-500/30 dark:bg-amber-500/10">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" aria-hidden />
        <p>
          These features are unfinished. They may not work, and can change or go away in any update. Back up bottles
          you care about before trying them. Features marked "Not built yet" do nothing for now.
        </p>
      </div>
      <div className="divide-y divide-frost-200 rounded-2xl border border-frost-200 bg-white dark:divide-slate-deep-700 dark:border-slate-deep-700 dark:bg-slate-deep-900">
        {flags?.map((f) => (
          <label key={f.flag} className="flex cursor-pointer items-center gap-4 px-5 py-3.5">
            <span className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="flex flex-wrap items-center gap-1.5">
                <span className="text-sm font-medium">{f.title}</span>
                <RiskBadge risk={f.risk} />
                {!f.available && <span className={subtle}>Not built yet</span>}
              </span>
              <span className={subtle}>{f.description}</span>
              <code className={`${subtle} font-mono`}>{f.flag}</code>
            </span>
            <Switch checked={f.enabled} onChange={(on) => toggle(f, on)} />
          </label>
        ))}
      </div>
      {setFlag.error && <ErrorText>{errorMessage(setFlag.error)}</ErrorText>}
      {confirming && (
        <Dialog onClose={() => setConfirming(undefined)}>
          <div className="flex flex-col gap-4">
            <h2 className="text-lg font-semibold">Turn on “{confirming.title}”?</h2>
            <p className="text-sm">{confirming.description}</p>
            <p className="text-sm">
              This is a high-risk experimental feature. It could leave a bottle or an app in a broken state. Back up
              bottles you care about first.
            </p>
            <div className="flex justify-end gap-2">
              <Button onClick={() => setConfirming(undefined)}>Cancel</Button>
              <Button
                variant="primary"
                onClick={() => {
                  setFlag.mutate({ flag: confirming.flag, enabled: true });
                  setConfirming(undefined);
                }}
              >
                Turn On
              </Button>
            </div>
          </div>
        </Dialog>
      )}
    </section>
  );
}

/** Disk usage, with a way to remove leftover downloads. */
function StorageGroup() {
  const queryClient = useQueryClient();
  const { data: usage, isFetching } = useQuery({
    queryKey: ["storage"],
    queryFn: () => commands.storageUsage(),
    staleTime: 0,
  });
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();

  const clear = async () => {
    setError(undefined);
    try {
      const freed = await commands.storageClearDownloads();
      setMessage(`Freed ${formatBytes(freed)}.`);
      await queryClient.invalidateQueries({ queryKey: ["storage"] });
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <Group title="Storage">
      {!usage ? (
        <p className={`px-5 py-3.5 ${subtle}`}>{isFetching ? "Measuring…" : ""}</p>
      ) : (
        <>
          {usage.bottles.map((b) => (
            <SizeRow key={b.id} title={b.name} detail="Bottle" bytes={b.bytes} />
          ))}
          <SizeRow title="Wine runtimes" bytes={usage.runtimes} />
          {usage.components > 0 && <SizeRow title="Graphics components" bytes={usage.components} />}
          <SizeRow
            title="Leftover downloads"
            detail={message ?? "Installers and archives from setups that stopped."}
            bytes={usage.downloads}
          >
            <Button disabled={usage.downloads === 0} onClick={clear}>
              <Trash2 className="size-4" /> Remove
            </Button>
          </SizeRow>
        </>
      )}
      {error && <div className="px-5 py-3"><ErrorText>{error}</ErrorText></div>}
    </Group>
  );
}

function SizeRow({
  title,
  detail,
  bytes,
  children,
}: {
  title: string;
  detail?: string;
  bytes: number;
  children?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 px-5 py-3">
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-sm font-medium">{title}</span>
        {detail && <span className={subtle}>{detail}</span>}
      </span>
      <span className="text-sm tabular-nums text-slate-deep-700/80 dark:text-frost-300/80">{formatSize(bytes)}</span>
      {children}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes > 0 && bytes < 1e6) return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
  return formatBytes(bytes);
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 text-sm font-medium">{title}</h2>
      <div className="divide-y divide-frost-200 rounded-2xl border border-frost-200 bg-white dark:divide-slate-deep-700 dark:border-slate-deep-700 dark:bg-slate-deep-900">
        {children}
      </div>
    </section>
  );
}

function Toggle({
  title,
  detail,
  checked,
  onChange,
}: {
  title: string;
  detail: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-4 px-5 py-3.5">
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-sm font-medium">{title}</span>
        <span className={subtle}>{detail}</span>
      </span>
      <Switch checked={checked} onChange={onChange} />
    </label>
  );
}

/** The on/off control inside a row's `<label>`. */
function Switch({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="peer sr-only"
      />
      <span
        aria-hidden
        className="relative h-5 w-9 shrink-0 rounded-full bg-frost-300 transition-colors peer-checked:bg-wine-500 peer-focus-visible:ring-2 peer-focus-visible:ring-wine-500/50 after:absolute after:top-0.5 after:left-0.5 after:size-4 after:rounded-full after:bg-white after:shadow after:transition-transform peer-checked:after:translate-x-4 dark:bg-slate-deep-700"
      />
    </>
  );
}
