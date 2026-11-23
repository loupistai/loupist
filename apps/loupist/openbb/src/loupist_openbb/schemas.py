"""Provider query metadata and nullable scalar output for bundled tool schemas."""

from __future__ import annotations

from types import UnionType
from typing import Any, Literal, Union, get_args, get_origin

# The IMF validators and ECB's non-null country type reject an omitted value,
# even though their model fields have a None default.
_REQUIRED_BY_VALIDATOR = {
    ("DirectionOfTrade", "imf"): ("country", "counterpart"),
    ("BalanceOfPayments", "ecb"): ("country",),
    ("EconomicIndicators", "imf"): ("symbol",),
}
_SCALARS = frozenset({"number", "integer", "string"})


def _literal_choices(annotation: Any) -> list[str]:
    if get_origin(annotation) in (UnionType, Union):
        base = [part for part in get_args(annotation) if part is not type(None)]
        if len(base) == 1:
            annotation = base[0]
    if get_origin(annotation) is Literal:
        choices = get_args(annotation)
        if choices and all(isinstance(choice, str) for choice in choices):
            return list(choices)
    return []


def apply_query_metadata(
    properties: dict[str, Any],
    model: str | None,
    provider: str,
    query_model: type,
) -> None:
    """Fill gaps left by OpenBB's merged schema from the selected fetcher."""
    required = _REQUIRED_BY_VALIDATOR.get((model, provider), ())
    for name, field in query_model.model_fields.items():
        if name not in properties:
            continue
        property_schema = properties[name]
        choices = _literal_choices(field.annotation)
        if choices and not property_schema.get(provider, {}).get("choices"):
            vendor = property_schema.setdefault(provider, {})
            vendor["choices"] = choices
            vendor["choices_from_query"] = True
        if field.is_required() or name in required:
            property_schema.setdefault(provider, {})["required"] = True

        if (model, provider) == ("DirectionOfTrade", "imf") and name in required:
            # The upstream text says None means all; the IMF fetcher refuses None.
            property_schema["description"] = (
                property_schema.get("description", "")
                .replace("None is an equiavlent to 'all'. ", "")
                .replace("None is equiavlent to 'all'. ", "")
                + " The imf provider needs both country and counterpart: ISO3 codes or"
                " country names, several separated by commas; one of the two may be all."
            ).strip()


def allow_nullable_output_scalars(schema: dict[str, Any] | bool) -> None:
    """Allow missing measures encoded as null without changing other constraints."""
    if isinstance(schema, bool):
        return
    kind = schema.get("type")
    if isinstance(kind, str) and kind in _SCALARS:
        schema["type"] = [kind, "null"]
    elif isinstance(kind, list) and "null" not in kind and _SCALARS.intersection(kind):
        schema["type"] = [*kind, "null"]
    for key in ("properties", "patternProperties", "$defs", "definitions"):
        for child in schema.get(key, {}).values():
            allow_nullable_output_scalars(child)
    for key in ("items", "additionalProperties"):
        child = schema.get(key)
        if isinstance(child, dict):
            allow_nullable_output_scalars(child)
    for key in ("anyOf", "allOf", "prefixItems"):
        for child in schema.get(key, ()):
            allow_nullable_output_scalars(child)
