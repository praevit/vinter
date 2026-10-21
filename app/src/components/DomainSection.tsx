import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, KeyRound, LoaderCircle, Trash2 } from "lucide-react";
import { commands, type DomainAccount, type Runtime } from "../api/bindings";
import type { BottleDetails } from "../api/bottles";
import { errorMessage } from "../api/errors";
import { Button, ErrorText, Label, field, subtle } from "./ui";

const empty: Required<DomainAccount> = {
  user: "",
  domain: "",
  dns_domain: "",
  computer_name: "",
  kdc: "",
};

/** The bottle's Windows domain account: who its programs run as, and Kerberos sign-in. */
export function DomainSection({
  bottle,
  runtime,
  os,
}: {
  bottle: BottleDetails;
  runtime?: Runtime;
  os?: string;
}) {
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: ["bottles", bottle.id, "domain"],
    queryFn: () => commands.bottleDomainStatus(bottle.id),
  });
  const saved = bottle.domain;
  const [enabled, setEnabled] = useState(saved !== null);
  const [account, setAccount] = useState<Required<DomainAccount>>({ ...empty, ...saved });
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<"save" | "remove" | "sign-in">();
  const [error, setError] = useState<string>();
  const [result, setResult] = useState<string>();

  const set = (key: keyof DomainAccount) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setAccount({ ...account, [key]: e.target.value });

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ["bottles", bottle.id] }),
      queryClient.invalidateQueries({ queryKey: ["bottles", bottle.id, "domain"] }),
    ]);

  const run = async (kind: typeof busy, action: () => Promise<unknown>, done: string) => {
    setBusy(kind);
    setError(undefined);
    setResult(undefined);
    try {
      await action();
      setResult(done);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(undefined);
    }
  };

  const save = () =>
    run(
      "save",
      async () => {
        const updated = await commands.bottleSetDomain(bottle.id, account, password || null);
        if (updated.domain) setAccount({ ...empty, ...updated.domain });
        setPassword("");
        await refresh();
      },
      "Saved. Programs use the account the next time they start.",
    );

  const remove = () =>
    run(
      "remove",
      async () => {
        await commands.bottleSetDomain(bottle.id, null, null);
        setAccount(empty);
        setPassword("");
        setEnabled(false);
        await refresh();
      },
      "Removed the domain account and its saved password.",
    );

  const signIn = () =>
    run("sign-in", () => commands.bottleDomainSignIn(bottle.id), "Signed in. The domain accepted the account.");

  const hasPassword = status.data?.has_password ?? false;
  const complete = account.user.trim() && account.domain.trim() && account.dns_domain.trim();

  return (
    <section className="flex flex-col gap-3 border-t border-frost-200 pt-5 dark:border-slate-deep-700">
      <div>
        <h2 className="text-sm font-medium">Windows domain</h2>
        <p className={subtle}>
          Run this bottle's programs as a domain user: for apps that check who and where they run,
          or that sign in to domain servers with Windows sign-in (Kerberos). The Mac itself does not
          join the domain.
        </p>
      </div>

      {!enabled ? (
        <div>
          <Button onClick={() => setEnabled(true)}>
            <KeyRound className="size-4" /> Use a domain account
          </Button>
        </div>
      ) : (
        <>
          {runtime && !runtime.kerberos && (
            <p className="text-sm text-amber-700 dark:text-amber-300">
              {runtime.name} can't sign in to domain servers. Pick a Vinter Wine runtime that
              supports it under Runtime above.
            </p>
          )}
          {runtime && !runtime.domain_identity && (
            <p className="text-sm text-amber-700 dark:text-amber-300">
              With {runtime.name}, programs see the user name but not the domain or computer name.
            </p>
          )}
          <div className="grid gap-3 sm:grid-cols-2">
            <Label title="User name">
              <input className={field} value={account.user} placeholder="jdoe" maxLength={20} autoComplete="off" spellCheck={false} onChange={set("user")} />
            </Label>
            <Label title={hasPassword ? "Password (saved)" : "Password"}>
              <input
                className={field}
                type="password"
                value={password}
                placeholder={hasPassword ? "Leave empty to keep it" : ""}
                autoComplete="off"
                onChange={(e) => setPassword(e.target.value)}
              />
            </Label>
            <Label title="Domain">
              <input className={field} value={account.domain} placeholder="CORP" maxLength={15} autoComplete="off" spellCheck={false} onChange={set("domain")} />
            </Label>
            <Label title="DNS domain">
              <input className={field} value={account.dns_domain} placeholder="corp.example.com" autoComplete="off" spellCheck={false} onChange={set("dns_domain")} />
            </Label>
            <Label title="Computer name (optional)">
              <input className={field} value={account.computer_name} placeholder="Keep the Mac's" maxLength={15} autoComplete="off" spellCheck={false} onChange={set("computer_name")} />
            </Label>
            <Label title="Domain controller (optional)">
              <input className={field} value={account.kdc} placeholder="Found through DNS" autoComplete="off" spellCheck={false} onChange={set("kdc")} />
            </Label>
          </div>
          <p className={subtle}>
            The password is kept in your {os === "macos" ? "Mac's Keychain" : "keyring"},
            not in the bottle, and is used to sign in each time a program starts. Signing in needs the
            Mac to reach a domain controller, for example over VPN. Close the bottle's programs before
            saving.
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="primary" disabled={!complete || busy !== undefined} onClick={save}>
              {busy === "save" ? "Saving…" : "Save"}
            </Button>
            {saved && (
              <Button disabled={!hasPassword || busy !== undefined} onClick={signIn}>
                {busy === "sign-in" ? <LoaderCircle className="size-4 animate-spin" /> : <CheckCircle2 className="size-4" />}
                Test sign-in
              </Button>
            )}
            {saved ? (
              <Button variant="danger" disabled={busy !== undefined} onClick={remove}>
                <Trash2 className="size-4" /> Remove account
              </Button>
            ) : (
              <Button disabled={busy !== undefined} onClick={() => setEnabled(false)}>
                Cancel
              </Button>
            )}
          </div>
        </>
      )}
      {result && <p className={subtle}>{result}</p>}
      {error && <ErrorText><span className="whitespace-pre-wrap">{error}</span></ErrorText>}
    </section>
  );
}
