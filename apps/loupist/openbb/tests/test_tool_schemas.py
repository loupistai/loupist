import asyncio

from fastapi import FastAPI, Response
from fastmcp import Client
from jsonschema import Draft202012Validator
from openbb_core.api.app_loader import AppLoader
from openbb_core.app.model.abstract.error import OpenBBError
from openbb_core.provider.utils.errors import EmptyDataError
from openbb_mcp_server.app.app import create_mcp_server
from openbb_mcp_server.models.settings import MCPSettings
from pydantic import BaseModel

from loupist_openbb.schemas import allow_nullable_output_scalars
from loupist_openbb.server import (
    mark_registered_tools_read_only,
    retain_read_only_routes,
)


def test_nullable_scalar_output_preserves_other_constraints() -> None:
    schema = {
        "type": "object",
        "properties": {
            "rows": {"type": "array", "items": {"$ref": "#/$defs/row"}},
        },
        "required": ["rows"],
        "additionalProperties": False,
        "$defs": {
            "row": {
                "type": "object",
                "required": ["ratio", "value", "label", "state"],
                "properties": {
                    "ratio": {"type": "number", "minimum": 0},
                    "value": {"type": "integer"},
                    "label": {"type": "string", "minLength": 2},
                    "state": {"type": "string", "enum": ["ok"]},
                },
            }
        },
        "examples": [{"type": "string"}],
    }
    allow_nullable_output_scalars(schema)
    validator = Draft202012Validator(schema)
    row = {"ratio": None, "value": None, "label": None, "state": "ok"}
    assert validator.is_valid({"rows": [row]})
    for invalid in (
        {},
        {"rows": None},
        {"rows": [None]},
        {"rows": [dict(row, ratio="0")]},
        {"rows": [dict(row, ratio=-1)]},
        {"rows": [dict(row, label="a")]},
        {"rows": [dict(row, value=1.5)]},
        {"rows": [dict(row, state=None)]},
        {"rows": [row], "extra": 1},
    ):
        assert not validator.is_valid(invalid), invalid
    assert schema["examples"] == [{"type": "string"}]


def test_openapi_tool_delivers_nulls_and_reports_empty_answers() -> None:
    class Metrics(BaseModel):
        ratio: float
        label: str

    class Output(BaseModel):
        results: list[Metrics]

    app = FastAPI()
    AppLoader.add_exception_handlers(app)

    @app.get("/api/v1/equity/estimates/forward_eps", response_model=Output)
    async def estimate(provider: str, mode: str):
        if mode == "no_content":
            return Response(status_code=204)
        if mode == "empty_error":
            raise EmptyDataError()
        if mode == "seeking_alpha_empty":
            raise OpenBBError("No estimates data was returned for: TSLA")
        if mode == "empty_rows":
            return {"results": []}
        if mode == "bad_type":
            return Response(
                '{"results":[{"ratio":"bad","label":null}]}',
                media_type="application/json",
            )
        return Response(
            '{"results":[{"ratio":null,"label":null}]}',
            media_type="application/json",
        )

    server = create_mcp_server(
        MCPSettings(
            name="Output contract",
            default_tool_categories=[],
            allowed_tool_categories=["equity"],
            enable_tool_discovery=True,
            describe_responses=False,
            mask_error_details=True,
        ),
        retain_read_only_routes(app, {"equity"}, "/api/v1"),
    )
    mark_registered_tools_read_only(server, {})

    async def exercise() -> None:
        name = "equity_estimates_forward_eps"
        async with Client(server) as client:
            await client.call_tool("activate_tools", {"tool_names": [name]})
            for mode in (
                "no_content",
                "empty_error",
                "seeking_alpha_empty",
                "empty_rows",
            ):
                result = await client.call_tool(
                    name,
                    {"provider": "seeking_alpha", "mode": mode},
                    raise_on_error=False,
                )
                assert result.is_error, mode
                assert result.structured_content == {
                    "loupist_error": {
                        "kind": "empty_result",
                        "provider": "seeking_alpha",
                    }
                }, mode
            result = await client.call_tool(
                name, {"provider": "fixture", "mode": "null"}
            )
            assert result.structured_content == {
                "results": [{"ratio": None, "label": None}]
            }
            invalid = await client.call_tool(
                name, {"provider": "fixture", "mode": "bad_type"}, raise_on_error=False
            )
            assert invalid.is_error
            assert "Output validation error" in invalid.content[0].text

    asyncio.run(exercise())
