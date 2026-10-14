"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { ArrowRight, Layers3 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export default function InvitationPage({ params }: { params: Promise<{ token: string }> }) {
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function accept(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError("");
    const form = new FormData(event.currentTarget);
    if (form.get("password") !== form.get("confirm_password")) { setError("Those passwords do not match."); setBusy(false); return; }
    try {
      const { token } = await params;
      const csrfResponse = await fetch("/api/v1/auth/csrf", { credentials: "include", cache: "no-store" });
      const csrf = ((await csrfResponse.json()) as { csrf_token?: string }).csrf_token || "";
      const response = await fetch(`/api/v1/invitations/${encodeURIComponent(token)}/accept`, { method: "POST", credentials: "include", headers: { "Content-Type": "application/json", "X-CSRF-Token": csrf }, body: JSON.stringify({ display_name: form.get("display_name"), password: form.get("password") }) });
      const data = (await response.json().catch(() => ({}))) as { error?: string };
      if (!response.ok) throw new Error(data.error || "This invitation could not be accepted.");
      setDone(true);
    } catch (e) { setError(e instanceof Error ? e.message : "This invitation could not be accepted."); }
    finally { setBusy(false); }
  }

  return <main className="flex min-h-svh items-center justify-center bg-muted/40 px-5 py-10"><Card className="w-full max-w-md"><CardHeader><div className="mb-2 grid size-10 place-items-center rounded-xl bg-primary text-primary-foreground"><Layers3 size={20} /></div><CardTitle>{done ? "You’re in" : "Accept workspace invitation"}</CardTitle><CardDescription>{done ? "Your account has been added to the workspace. Sign in to continue." : "Set your display name and password to join. If you already have an account, use its current password."}</CardDescription></CardHeader><CardContent>{done ? <Link href="/" className="inline-flex h-10 w-full items-center justify-center gap-2 rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground">Go to sign in <ArrowRight size={15} /></Link> : <form className="space-y-4" onSubmit={accept}><div className="space-y-2"><Label htmlFor="display-name">Display name</Label><Input id="display-name" name="display_name" required minLength={1} maxLength={80} autoComplete="name" /></div><div className="space-y-2"><Label htmlFor="password">Password</Label><Input id="password" name="password" type="password" required minLength={12} maxLength={128} autoComplete="new-password" /></div><div className="space-y-2"><Label htmlFor="confirm-password">Confirm password</Label><Input id="confirm-password" name="confirm_password" type="password" required minLength={12} maxLength={128} autoComplete="new-password" /></div>{error && <p role="alert" className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700">{error}</p>}<Button className="w-full" disabled={busy}>{busy ? "Accepting…" : "Accept invitation"}</Button></form>}</CardContent></Card></main>;
}
