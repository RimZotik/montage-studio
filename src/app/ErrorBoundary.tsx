import { RotateCw, TriangleAlert } from "lucide-react";
import { Component, type ErrorInfo, type ReactNode } from "react";
import i18n from "@/shared/i18n";
import { commands as ipc } from "@/shared/ipc/bindings";

/** Ошибка интерфейса — в журнал студии (виден в окошке ошибок и Claude по кнопке). */
export function logUiError(message: string) {
  void ipc.uiLog("error", message.slice(0, 4000)).catch(() => {});
}

/** Перехват ошибок за пределами React: необработанные исключения и обещания. */
export function installErrorLogging() {
  window.addEventListener("error", (e) => logUiError(`${e.message} @ ${e.filename}:${e.lineno}:${e.colno}${e.error?.stack ? `\n${e.error.stack}` : ""}`));
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason;
    logUiError(`unhandled rejection: ${r instanceof Error ? `${r.message}\n${r.stack ?? ""}` : String(r)}`);
  });
}

type State = { error: Error | null };

/** Страница упала — вместо белого экрана карточка с причиной и перезагрузкой страницы. */
export class ErrorBoundary extends Component<{ children: ReactNode; name: string }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    logUiError(`page ${this.props.name} crashed: ${error.message}\n${error.stack ?? ""}\ncomponents:${info.componentStack ?? ""}`);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="crash">
        <TriangleAlert size={22} strokeWidth={1.5} />
        <b>{i18n.t("crash.title")}</b>
        <code>{this.state.error.message}</code>
        <span className="faint">{i18n.t("crash.logged")}</span>
        <button className="btn pri" onClick={() => this.setState({ error: null })}>
          <RotateCw size={14} strokeWidth={1.75} />
          {i18n.t("crash.retry")}
        </button>
      </div>
    );
  }
}
