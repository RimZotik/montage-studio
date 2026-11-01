import { ArrowUp, Check, CircleAlert, FilePen, FileSearch, FolderSearch, Loader, Pencil, Plus, Square, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import Markdown from "react-markdown";
import { useTranslation } from "react-i18next";
import { type AppError, type ChatItem, commands as ipc, events } from "@/shared/ipc/bindings";
import { errorText } from "@/shared/lib/format";
import { type Ask, type Proposal, parseDecision, splitBlocks } from "./blocks";

type Turn = { ok: boolean; interrupted: boolean; durationMs: number; inputTokens: number; outputTokens: number; costUsd: number | null; denials: string[] };
type Item =
  | { k: "user"; text: string }
  | { k: "assistant"; text: string; streaming?: boolean }
  | { k: "tool"; id: string; name: string; summary: string; state: "run" | "ok" | "err" }
  | ({ k: "turn" } & Turn)
  | { k: "decision"; text: string }
  | { k: "error"; text: string };

/** Правил ли Claude таймлайн в ходе, который закончился строкой `end`. */
function turnEdited(items: Item[], end: number): boolean {
  for (let j = end - 1; j >= 0; j--) {
    const y = items[j];
    if (!y || y.k === "user") return false;
    if (y.k === "tool" && y.name === "mcp__studio__timeline_apply") return true;
  }
  return false;
}

/** История с диска → строки чата. */
function fromHistory(list: ChatItem[]): Item[] {
  return list.flatMap((x): Item[] => {
    if (x.role === "user") return [{ k: "user", text: x.text }];
    if (x.role === "assistant") return [{ k: "assistant", text: x.text }];
    if (x.role === "decision") return [{ k: "decision", text: x.text }];
    if (x.role === "tool") {
      const [name = "", ...rest] = x.text.split(" ");
      return [{ k: "tool", id: "", name, summary: rest.join(" "), state: "ok" }];
    }
    if (x.role === "turn") {
      try {
        return [{ k: "turn", ...(JSON.parse(x.text) as Turn) }];
      } catch {
        return [];
      }
    }
    return [];
  });
}

const TOOL_ICON: Record<string, ReactNode> = {
  Read: <FileSearch size={12} strokeWidth={1.75} />,
  Glob: <FolderSearch size={12} strokeWidth={1.75} />,
  Grep: <FolderSearch size={12} strokeWidth={1.75} />,
  Edit: <FilePen size={12} strokeWidth={1.75} />,
  Write: <FilePen size={12} strokeWidth={1.75} />,
};

function ProposalCard({ p, verdict, onDecide }: { p: Proposal; verdict?: string; onDecide: (text: string) => void }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState([p.description ?? "", ...(p.details ?? [])].filter(Boolean).join("\n"));
  const [note, setNote] = useState("");
  return (
    <div className={`card prop ${verdict ? "done" : ""}`}>
      <div className="ck">{t(`chat.kind.${p.kind}`, { defaultValue: t("chat.kind.plan") })}</div>
      <div className="ct">{p.title}</div>
      {editing ? (
        <textarea className="field area" value={draft} onChange={(e) => setDraft(e.target.value)} rows={5} />
      ) : (
        <>
          {p.description && <div className="cd">{p.description}</div>}
          {p.details && p.details.length > 0 && (
            <ul className="cl">
              {p.details.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          )}
        </>
      )}
      {verdict ? (
        <div className={`cv ${verdict}`}>{t(`chat.verdict.${verdict}`, { defaultValue: verdict })}</div>
      ) : editing ? (
        <div className="cb">
          <button className="btn pri" onClick={() => onDecide(`[proposal ${p.id}: edited] ${draft}`)}>
            {t("chat.sendEdits")}
          </button>
          <button className="btn quiet" onClick={() => setEditing(false)}>
            {t("newProject.cancel")}
          </button>
        </div>
      ) : (
        <>
          <div className="cb">
            <button className="btn pri" onClick={() => onDecide(`[proposal ${p.id}: accepted]`)}>
              <Check size={13} strokeWidth={2} />
              {t("chat.accept")}
            </button>
            <button className="btn" onClick={() => setEditing(true)}>
              <Pencil size={13} strokeWidth={1.75} />
              {t("chat.edit")}
            </button>
            <button className="btn quiet" onClick={() => onDecide(`[proposal ${p.id}: rejected]`)}>
              {t("chat.reject")}
            </button>
          </div>
          <input
            className="field note"
            placeholder={t("chat.writeEdits")}
            value={note}
            onChange={(e) => setNote(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && note.trim() && onDecide(`[proposal ${p.id}: comment] ${note.trim()}`)}
          />
        </>
      )}
    </div>
  );
}

function AskCard({ a, answer, onDecide }: { a: Ask; answer?: string; onDecide: (text: string) => void }) {
  return (
    <div className="card ask">
      <div className="ct">{a.question}</div>
      <div className="cb wrap">
        {a.options.map((o) => (
          <button key={o} className={`btn ${answer === o ? "chosen" : ""}`} disabled={!!answer} onClick={() => onDecide(`[answer ${a.id}] ${o}`)}>
            {o}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Панель чата с Claude в открытом проекте (docs/claude-bridge.md). */
export function ChatPanel({ projectPath, onError, width }: { projectPath: string; onError: (e: AppError) => void; width: number }) {
  const { t, i18n } = useTranslation();
  const [items, setItems] = useState<Item[]>([]);
  const [busy, setBusy] = useState(false);
  const [model, setModel] = useState<string | null>(null);
  const [text, setText] = useState("");
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void ipc.chatHistory().then((h) => setItems(fromHistory(h)));
    void ipc.chatBusy().then(setBusy);
  }, [projectPath]);

  useEffect(() => {
    const off = events.chatEvent.listen(({ payload: e }) => {
      setItems((all) => {
        const last = all[all.length - 1];
        switch (e.kind) {
          case "delta":
            if (last?.k === "assistant" && last.streaming) return [...all.slice(0, -1), { ...last, text: last.text + e.text }];
            return [...all, { k: "assistant", text: e.text, streaming: true }];
          case "assistant":
            if (last?.k === "assistant" && last.streaming) return [...all.slice(0, -1), { k: "assistant", text: e.text }];
            return [...all, { k: "assistant", text: e.text }];
          case "tool":
            return [...all, { k: "tool", id: e.id, name: e.name, summary: e.summary, state: "run" }];
          case "toolDone":
            return all.map((x) => (x.k === "tool" && x.id === e.id ? { ...x, state: e.error ? "err" : "ok" } : x));
          case "turn":
            return [...all.map((x) => (x.k === "assistant" && x.streaming ? { ...x, streaming: false } : x)), { k: "turn", ...e }];
          default:
            return all;
        }
      });
      if (e.kind === "ready") setModel(e.model);
      if (e.kind === "turn") setBusy(false);
    });
    return () => void off.then((f) => f());
  }, []);

  useEffect(() => {
    list.current?.scrollTo({ top: list.current.scrollHeight });
  }, [items]);

  const decisions = useMemo(() => {
    const map = new Map<string, string>();
    for (const x of items) if (x.k === "decision") {
      const d = parseDecision(x.text);
      if (d) map.set(d.id, d.verdict);
    }
    return map;
  }, [items]);

  const send = useCallback(
    async (message: string, kind: "user" | "decision" = "user") => {
      const body = message.trim();
      if (!body) return;
      setItems((all) => [...all, kind === "decision" ? { k: "decision", text: body } : { k: "user", text: body }]);
      setBusy(true);
      const r = await ipc.chatSend(body, kind);
      if (r.status === "error") {
        setBusy(false);
        onError(r.error);
        setItems((all) => [...all, { k: "error", text: errorText(r.error) }]);
      }
    },
    [onError],
  );

  const newSession = async () => {
    const r = await ipc.chatNewSession();
    if (r.status === "error") onError(r.error);
    else {
      setItems([]);
      setModel(null);
      setBusy(false);
    }
  };

  const fmt = (n: number) => n.toLocaleString(i18n.language);

  return (
    <aside className="chat" style={{ width }}>
      <div className="chathead">
        <b>Claude</b>
        {model && <span className="model">{model.replace(/^claude-/, "")}</span>}
        <button className="iconbtn" title={t("chat.newSession")} onClick={() => void newSession()}>
          <Plus size={15} strokeWidth={1.75} />
        </button>
      </div>

      <div className="msgs" ref={list}>
        {items.length === 0 && (
          <div className="chatempty">
            <div>{t("chat.empty")}</div>
            {(["chat.try1", "chat.try2"] as const).map((k) => (
              <button key={k} className="chip" onClick={() => void send(t(k))}>
                {t(k)}
              </button>
            ))}
          </div>
        )}
        {items.map((x, i) => {
          const key = `${i}-${x.k}`;
          if (x.k === "user") return <div key={key} className="m-user">{x.text}</div>;
          if (x.k === "decision" || x.k === "error") return x.k === "error" ? (
            <div key={key} className="m-err ic">
              <CircleAlert size={13} strokeWidth={1.75} />
              {x.text}
            </div>
          ) : null;
          if (x.k === "tool")
            return (
              <div key={key} className={`m-tool ${x.state}`} title={x.summary}>
                {x.state === "run" ? <Loader size={12} strokeWidth={1.75} className="spin" /> : x.state === "err" ? <X size={12} strokeWidth={1.75} /> : TOOL_ICON[x.name] ?? <Check size={12} strokeWidth={1.75} />}
                <span>{t(`chat.tool.${x.name}`, { defaultValue: x.name })}</span>
                <span className="mono">{x.summary}</span>
              </div>
            );
          if (x.k === "turn")
            return (
              <div key={key}>
                {x.denials.length > 0 && (
                  <div className="card deny">
                    <div className="ct ic">
                      <CircleAlert size={13} strokeWidth={1.75} />
                      {t("chat.denied")}
                    </div>
                    {x.denials.map((d) => (
                      <div key={d} className="mono">
                        {d}
                      </div>
                    ))}
                  </div>
                )}
                {x.ok && !x.interrupted && i === items.length - 1 && turnEdited(items, i) && (
                  <button className="link undoturn" onClick={() => void ipc.projectUndoClaude().then((r) => r.status === "error" && onError(r.error))}>
                    {t("chat.undoTurn")}
                  </button>
                )}
                <div className="m-turn">
                  {x.interrupted ? t("chat.stopped") : `${(x.durationMs / 1000).toFixed(1)} ${t("chat.sec")} · ${fmt(Math.round((x.inputTokens + x.outputTokens) / 1000))}k ${t("chat.tokens")}${x.costUsd ? ` · ≈${x.costUsd.toFixed(3)} $` : ""}`}
                </div>
              </div>
            );
          return (
            <div key={key} className="m-ai">
              {splitBlocks(x.text).map((part, j) =>
                part.type === "text" ? (
                  <Markdown key={j}>{part.text}</Markdown>
                ) : part.type === "proposal" ? (
                  <ProposalCard key={part.value.id} p={part.value} verdict={decisions.get(part.value.id)} onDecide={(d) => void send(d, "decision")} />
                ) : (
                  <AskCard key={part.value.id} a={part.value} answer={decisions.get(part.value.id)} onDecide={(d) => void send(d, "decision")} />
                ),
              )}
              {x.streaming && <span className="caret" />}
            </div>
          );
        })}
      </div>

      <div className="input">
        <textarea
          placeholder={t("chat.placeholder")}
          value={text}
          rows={3}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void send(text);
              setText("");
            }
          }}
        />
        <div className="inrow">
          <span className="faint">{t("chat.hint")}</span>
          {busy ? (
            <button className="sendbtn stop" title={t("chat.stop")} onClick={() => void ipc.chatStop().then(() => setBusy(false))}>
              <Square size={11} strokeWidth={2.5} />
            </button>
          ) : (
            <button
              className="sendbtn"
              disabled={!text.trim()}
              onClick={() => {
                void send(text);
                setText("");
              }}
            >
              <ArrowUp size={15} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>
    </aside>
  );
}
