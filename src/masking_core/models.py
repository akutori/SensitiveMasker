from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field, model_validator


class Rule(BaseModel):
    name: str
    pattern_type: Literal["literal", "regex"]
    pattern: str
    mode: Literal["fixed", "random"]
    fixed_value: str | None = None
    prefix: str | None = None
    enabled: bool = True
    description: str | None = None

    @model_validator(mode="after")
    def check_mode_requirements(self) -> Rule:
        if self.mode == "fixed" and not self.fixed_value:
            raise ValueError(
                f"Rule '{self.name}': mode='fixed' requires a non-empty 'fixed_value'"
            )
        if self.mode == "random" and not self.prefix:
            raise ValueError(
                f"Rule '{self.name}': mode='random' requires a non-empty 'prefix'"
            )
        return self


class RuleProfile(BaseModel):
    profile_name: str
    description: str | None = None
    rules: list[Rule] = Field(default_factory=list)
