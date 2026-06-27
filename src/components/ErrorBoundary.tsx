import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";

type Props = { children: ReactNode };
type State = { error: Error | null };

/** Surfaces render errors on-screen instead of a black screen. */
export default class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("UI error:", error, info);
  }
  render() {
    const e = this.state.error;
    if (e) {
      return (
        <div className="p-6 text-neutral-200">
          <h2 className="text-sm font-semibold text-red-400">
            UI crashed — paste this to Nova
          </h2>
          <pre className="mt-2 whitespace-pre-wrap text-xs text-neutral-400">
            {e.message}
            {"\n"}
            {e.stack}
          </pre>
          <button
            className="mt-3 rounded bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:bg-neutral-700"
            onClick={() => this.setState({ error: null })}
          >
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
