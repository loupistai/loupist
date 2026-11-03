import i18n from "@/i18n";
import { Component, createRef, type ReactNode } from "react";

type Props = {
  children: ReactNode;
  name: string;
  resetKey?: string;
  root?: boolean;
};

/** Keep a failed renderer surface from taking the navigation and other views down. */
export class ViewBoundary extends Component<Props, { failed: boolean }> {
  state = { failed: false };
  private retryButton = createRef<HTMLButtonElement>();

  static getDerivedStateFromError() {
    return { failed: true };
  }

  private onLanguageChanged = () => this.forceUpdate();

  componentDidMount() { i18n.on("languageChanged", this.onLanguageChanged); }

  componentWillUnmount() { i18n.off("languageChanged", this.onLanguageChanged); }

  componentDidCatch() {
    this.retryButton.current?.focus();
  }

  componentDidUpdate(previous: Props) {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <section className="view-error" role="alert" aria-label={i18n.t("app:viewBoundary.unavailable", { name: this.props.name })}>
        <h1>{i18n.t("app:viewBoundary.couldNotDisplay", { name: this.props.name })}</h1>
        <p>{this.props.root
          ? i18n.t("app:viewBoundary.reloadTheAppToTryAgainYourSaved")
          : i18n.t("app:viewBoundary.tryAgainOrUseTheSidebarToOpen")}</p>
        <button ref={this.retryButton} type="button" className="primary-button"
          onClick={() => this.props.root ? window.location.reload() : this.setState({ failed: false })}>
          {this.props.root ? i18n.t("app:viewBoundary.reloadApp") : i18n.t("app:viewBoundary.tryAgain")}
        </button>
      </section>
    );
  }
}
