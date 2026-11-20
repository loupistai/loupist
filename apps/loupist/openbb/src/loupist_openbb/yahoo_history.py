"""Identify Yahoo daily chart prices that are still on their as-traded share basis."""

from contextvars import ContextVar
from datetime import date
from functools import wraps
from typing import Any

from loupist_openbb.failures import YahooAdjustmentUnavailable

RAW_DAILY_REQUEST: ContextVar[dict[str, Any] | None] = ContextVar(
    "yahoo_raw_daily_request", default=None
)


def apply_yahoo_history() -> None:
    import yfinance as yf

    original = yf.download
    if getattr(original, "__loupist_yahoo_history__", False):
        return

    @wraps(original)
    def download(*args, **kwargs):
        request = RAW_DAILY_REQUEST.get()
        if request is None:
            return original(*args, **kwargs)

        # The pinned fetcher downloads through the present, then filters dates.
        # Keep subsequent split events visible until the share basis is checked.
        if kwargs.get("end") is not None or kwargs.get("interval") != "1d":
            raise YahooAdjustmentUnavailable()
        bars = original(*args, **{**kwargs, "actions": True})
        if bars.empty:
            return bars
        symbols = kwargs["tickers"].split(",")
        for symbol in symbols:
            series = bars
            if bars.columns.nlevels > 1:
                if (
                    "Ticker" not in bars.columns.names
                    or symbol not in bars.columns.get_level_values("Ticker")
                ):
                    raise YahooAdjustmentUnavailable()
                series = bars.xs(symbol, level="Ticker", axis=1)
            if "Stock Splits" not in series:
                raise YahooAdjustmentUnavailable()
            traded = series.index[series["Open"] > 0]
            if traded.empty:
                continue
            splits = series.loc[traded, "Stock Splits"]
            subsequent = series.loc[series.index > traded.min(), "Stock Splits"]
            if splits.isna().any() or subsequent.fillna(0).ne(0).any():
                raise YahooAdjustmentUnavailable()

        if end := request.get("end_date"):
            bars = bars[bars.index.date <= date.fromisoformat(end)]
        if not kwargs.get("actions"):
            level = "Price" if bars.columns.nlevels > 1 else None
            bars = bars.drop(
                columns=["Dividends", "Stock Splits", "Capital Gains"],
                level=level,
                errors="ignore",
            )
        return bars

    download.__loupist_yahoo_history__ = True
    yf.download = download
