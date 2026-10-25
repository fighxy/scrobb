import type { Command, ScrobbleStatus } from "../core";
import { deviceName } from "../core";
import type { LinkCode } from "../usePlayer";

interface Props {
  status: ScrobbleStatus | null;
  available: boolean;
  code: LinkCode | null;
  run: (command: Command) => void;
  openUrl: (url: string) => void;
}

/** Связь с аккаунтом Scrobb: привязка по коду (RFC 8628) и состояние отправки. */
export function ScrobbLink({ status, available, code, run, openUrl }: Props) {
  if (!available || !status) return null;
  const link = () => run({ type: "link_scrobb_account", data: { device_name: deviceName() } });
  const queued = status.pending > 0 ? ` · в очереди ${status.pending}` : "";

  switch (status.state) {
    case "not_linked":
      return (
        <button className="button primary" onClick={link}>
          Подключить Scrobb
        </button>
      );
    case "linking":
      return (
        <>
          <div className="chip">Привязка…</div>
          <div className="link-dialog" role="dialog" aria-label="Привязка к Scrobb">
          <div className="muted">Код для сайта Scrobb</div>
          <div className="code">{code?.user_code ?? "…"}</div>
          <p className="muted">
            Страница привязки открыта в браузере. Убедитесь, что код совпадает, и нажмите «Подключить».
          </p>
          <div className="dialog-actions">
            {code && (
              <button className="button" onClick={() => openUrl(code.verification_uri_complete)}>
                Открыть сайт
              </button>
            )}
            <button className="button" onClick={() => run({ type: "cancel_scrobb_link" })}>
              Отмена
            </button>
          </div>
          </div>
        </>
      );
    case "ready":
    case "retrying":
      return (
        <div className="chip" title={status.state === "retrying" ? "Нет связи — повторим позже" : "Прослушивания уходят в Scrobb"}>
          <span className={`dot ${status.state}`} />
          {status.state === "ready" ? "Scrobb" : "Scrobb · нет связи"}
          {queued}
          <button className="link" onClick={() => run({ type: "unlink_scrobb_account" })}>
            Отключить
          </button>
        </div>
      );
    case "unauthorized":
      return (
        <div className="chip warn">
          Scrobb: войдите заново{queued}
          <button className="link" onClick={link}>
            Подключить
          </button>
        </div>
      );
    case "upgrade_required":
      return <div className="chip warn">Обновите плеер, чтобы скробблить{queued}</div>;
  }
}
