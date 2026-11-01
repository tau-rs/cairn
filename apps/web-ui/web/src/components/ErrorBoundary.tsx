import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "./ui/Button";

type Props = {
  children: ReactNode;
  /** Diagnostic hook. Defaults to console.error. */
  onError?: (error: Error, info: ErrorInfo) => void;
  /**
   * Custom fallback. Receives a `reset` callback that clears the error state so
   * the boundary re-renders its children (use after the cause is resolved).
   * When omitted, the default full-app fallback (with a page reload) is shown.
   */
  fallback?: (reset: () => void) => ReactNode;
};

type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Log the error + component stack so a render throw leaves a diagnostic
    // trail instead of a silent blank window.
    if (this.props.onError) {
      this.props.onError(error, info);
    } else {
      console.error(
        "ErrorBoundary caught an error:",
        error,
        info.componentStack,
      );
    }
  }

  reset = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error === null) return this.props.children;
    if (this.props.fallback) return this.props.fallback(this.reset);
    return <DefaultFallback error={error} />;
  }
}

function DefaultFallback({ error }: { error: Error }) {
  return (
    <div
      role="alert"
      className="flex h-full min-h-[12rem] w-full flex-col items-center justify-center gap-4 bg-bg p-8 text-center text-text"
    >
      <div className="flex flex-col items-center gap-2">
        <h1 className="text-lg font-semibold">Something went wrong</h1>
        <p className="max-w-md text-sm text-muted">
          The app hit an unexpected error and can&apos;t continue. Reloading
          usually fixes it. If it keeps happening, the details below help with
          diagnosis.
        </p>
      </div>
      <Button variant="primary" onClick={() => window.location.reload()}>
        Reload
      </Button>
      <pre className="max-h-40 max-w-md overflow-auto rounded border border-border bg-surface px-3 py-2 text-left text-xs text-danger">
        {error.message}
      </pre>
    </div>
  );
}
