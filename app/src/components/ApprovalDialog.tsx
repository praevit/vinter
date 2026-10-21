import { useCallback, useEffect, useState } from "react";
import { Bot } from "lucide-react";
import { commands, events, type ApprovalRequest, type Decision } from "../api/bindings";
import { errorMessage } from "../api/errors";
import { Dialog } from "./Dialog";
import { Button, ErrorText, subtle } from "./ui";

/**
 * Asks the person to allow what an AI agent (through `vinter mcp`) wants to
 * do. Requests arrive through `vinter://approve/<id>`; one is shown at a time.
 */
export function ApprovalDialog() {
  const [pending, setPending] = useState<ApprovalRequest[]>([]);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    void commands.approvalsPending().then(setPending);
  }, []);

  useEffect(() => {
    refresh();
    const requested = events.appApproval.listen(refresh);
    window.addEventListener("focus", refresh);
    return () => {
      requested.then((u) => u());
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);

  const request = pending[0];
  if (!request) return null;

  const decide = async (decision: Decision) => {
    setBusy(true);
    setError(undefined);
    try {
      await commands.approvalDecide(request.id, decision);
    } catch (e) {
      setError(errorMessage(e));
    }
    setBusy(false);
    refresh();
  };

  return (
    <Dialog>
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <Bot className="size-5 text-wine-500" />
          <h2 className="font-medium">{request.client} wants to:</h2>
        </div>
        <p className="text-sm select-text">{request.summary}</p>
        <p className={subtle}>
          Vinter snapshots the bottle first, so you can undo this from its Settings tab.
          {pending.length > 1 && ` ${pending.length - 1} more request(s) waiting.`}
        </p>
        {error && <ErrorText>{error}</ErrorText>}
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="danger" disabled={busy} onClick={() => decide("deny")}>
            Deny
          </Button>
          <Button disabled={busy} onClick={() => decide("session")} title={`Until ${request.client} disconnects`}>
            Allow for this session
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => decide("once")}>
            Allow once
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
