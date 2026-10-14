import type { NextRequest } from "next/server";

export const dynamic = "force-dynamic";

type RouteContext = { params: Promise<{ path: string[] }> };

async function proxy(request: NextRequest, context: RouteContext) {
  const { path } = await context.params;
  const origin = process.env.API_INTERNAL_URL ?? "http://localhost:8080";
  const pathname = path.map(encodeURIComponent).join("/");
  const target = new URL(`/api/v1/${pathname}${request.nextUrl.search}`, origin);
  const headers = new Headers(request.headers);
  for (const name of ["host", "connection", "keep-alive", "transfer-encoding", "content-length", "accept-encoding"]) {
    headers.delete(name);
  }

  const hasBody = !["GET", "HEAD"].includes(request.method);
  const upstream = await fetch(target, {
    method: request.method,
    headers,
    body: hasBody ? await request.arrayBuffer() : undefined,
    cache: "no-store",
    redirect: "manual",
  });

  const responseHeaders = new Headers(upstream.headers);
  for (const name of ["connection", "keep-alive", "transfer-encoding", "content-length", "content-encoding"]) {
    responseHeaders.delete(name);
  }
  responseHeaders.delete("set-cookie");
  for (const cookie of upstream.headers.getSetCookie()) {
    responseHeaders.append("set-cookie", cookie);
  }

  return new Response(upstream.body, { status: upstream.status, headers: responseHeaders });
}

export const GET = proxy;
export const POST = proxy;
export const PATCH = proxy;
export const PUT = proxy;
export const DELETE = proxy;
export const OPTIONS = proxy;
