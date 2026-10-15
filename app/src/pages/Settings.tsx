import { useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { useAppInfo } from "../api/app";
import { commands, type Settings_Serialize as SettingsData } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { formatBytes, useInstalledRuntimes } from "../api/runtimes";
import { useSaveSettings, useSettings } from "../api/settings";
import { BottleOptionsFields } from "../components/BottleOptionsFields";
import { Select } from "../components/Select";
import { Button, ErrorText, subtle } from "../components/ui";

export function Settings() {
  const { data: view } = useSettings();
  const { data: app } = useAppInfo();
  const { data: runtimes } = useInstalledRuntimes();
  const save = useSaveSettings();
  const queryClient = useQueryClient();
  const [loginError, setLoginError] = useState<string>();

  if (!view) return null;
  const { launch_at_login, ...settings } = view;
  const set = (patch: Partial<SettingsData>) => save.mutate({ ...settings, ...patch });
  const mac = app?.os === "macos";

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
          detail="Vinter looks for a newer version when it starts and tells you. It never installs one by itself."
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

      <StorageGroup />
    </div>
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
    </label>
  );
}
