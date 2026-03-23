import type { Env } from "./types";

export class PairingIndexDO {
  constructor(
    private readonly _ctx: DurableObjectState,
    private readonly _env: Env,
  ) {}

  async fetch(): Promise<Response> {
    return Response.json(
      {
        error: "not_implemented",
        message: "PairingIndexDO is reserved for a later step; use /v1/dev/bootstrap and /v1/dev/pair for now.",
      },
      { status: 501 },
    );
  }
}
