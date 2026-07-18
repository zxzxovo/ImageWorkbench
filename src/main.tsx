import { ErrorBoundary } from "solid-js";
import { render } from "solid-js/web";
import App from "./App";
import "./styles.css";

function CrashRecovery(props: { error: unknown; reset: () => void }) {
  const message = props.error instanceof Error ? props.error.stack ?? props.error.message : String(props.error);
  const copy = () => {
    const report = JSON.stringify({
      occurredAt: new Date().toISOString(),
      location: window.location.href,
      userAgent: navigator.userAgent,
      error: message,
    }, null, 2);
    void navigator.clipboard?.writeText(report).catch(() => undefined);
  };

  return (
    <main class="crash-recovery" role="alert">
      <section>
        <span class="section-kicker">ImageWorkbench</span>
        <h1>界面发生错误 / Interface error</h1>
        <p>项目文件没有被删除。请复制错误信息后重试；若问题持续出现，请重新启动应用。</p>
        <pre>{message}</pre>
        <div>
          <button class="button secondary" type="button" onClick={copy}>复制错误 / Copy error</button>
          <button class="button primary" type="button" onClick={props.reset}>重试 / Retry</button>
          <button class="button secondary" type="button" onClick={() => window.location.reload()}>重新启动 / Reload</button>
        </div>
      </section>
    </main>
  );
}

render(() => (
  <ErrorBoundary fallback={(error, reset) => <CrashRecovery error={error} reset={reset} />}>
    <App />
  </ErrorBoundary>
), document.getElementById("root")!);
