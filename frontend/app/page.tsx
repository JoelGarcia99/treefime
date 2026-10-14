"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { ArrowRight, Check, CircleHelp, Layers3, LockKeyhole, LogOut, ShieldCheck, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type User = {
  id: string;
  email: string;
  display_name: string;
  is_root: boolean;
  must_update_credentials: boolean;
};

type ApiResult<T> = { response: Response; data: T };
type ErrorBody = { error?: string };

async function readResponse<T>(response: Response): Promise<ApiResult<T>> {
  const data = (await response.json().catch(() => ({}))) as T;
  return { response, data };
}

function getError(data: ErrorBody, fallback: string) {
  return data.error || fallback;
}

function BrandMark() {
  return <div className="grid size-10 place-items-center rounded-xl bg-primary text-primary-foreground shadow-sm"><Layers3 size={20} strokeWidth={2.5} /></div>;
}

export default function Home() {
  const [screen, setScreen] = useState<"loading" | "login" | "forced" | "workspace">("loading");
  const [user, setUser] = useState<User | null>(null);
  const [csrfToken, setCsrfToken] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const primeCsrf = useCallback(async () => {
    try {
      const { response, data } = await readResponse<{ csrf_token?: string }>(
        await fetch("/api/v1/auth/csrf", { credentials: "include", cache: "no-store" }),
      );
      if (response.ok && data.csrf_token) setCsrfToken(data.csrf_token);
    } catch {
      setScreen("login");
      setError("The API is unavailable. Start the backend and try again.");
    }
  }, []);

  const secureFetch = useCallback(async (path: string, init: RequestInit = {}) => {
    let token = csrfToken;
    if (!token) {
      const csrfResponse = await fetch("/api/v1/auth/csrf", { credentials: "include", cache: "no-store" });
      const csrfData = (await csrfResponse.json()) as { csrf_token?: string };
      token = csrfData.csrf_token || "";
      setCsrfToken(token);
    }
    const headers = new Headers(init.headers);
    headers.set("X-CSRF-Token", token);
    const response = await fetch(path, { ...init, headers, credentials: "include", cache: "no-store" });
    const refreshed = await response.clone().json().catch(() => ({})) as { csrf_token?: string };
    if (refreshed.csrf_token) setCsrfToken(refreshed.csrf_token);
    return response;
  }, [csrfToken]);

  const loadCurrentUser = useCallback(async () => {
    try {
      let response = await fetch("/api/v1/auth/me", { credentials: "include", cache: "no-store" });
      if (response.status === 401) {
        const refreshed = await secureFetch("/api/v1/auth/refresh", { method: "POST" });
        if (refreshed.ok) response = await fetch("/api/v1/auth/me", { credentials: "include", cache: "no-store" });
      }
      if (response.status === 403) {
        setScreen("forced");
        return;
      }
      if (response.ok) {
        const nextUser = (await response.json()) as User;
        setUser(nextUser);
        setScreen(nextUser.must_update_credentials ? "forced" : "workspace");
        return;
      }
      setScreen("login");
    } catch {
      setScreen("login");
      setError("The API is unavailable. Start the backend and try again.");
    }
  }, [secureFetch]);

  useEffect(() => {
    void (async () => {
      await primeCsrf();
      await loadCurrentUser();
    })();
  }, [loadCurrentUser, primeCsrf]);

  async function signIn(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError("");
    setBusy(true);
    const form = new FormData(event.currentTarget);
    try {
      const result = await readResponse<{ user: User; csrf_token?: string; error?: string }>(await secureFetch("/api/v1/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: form.get("email"), password: form.get("password"), device_name: "Browser" }),
      }));
      if (!result.response.ok) throw new Error(getError(result.data, "Could not sign in. Check your details and try again."));
      setUser(result.data.user);
      setScreen(result.data.user.must_update_credentials ? "forced" : "workspace");
      if (result.data.csrf_token) setCsrfToken(result.data.csrf_token);
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : "Could not sign in.");
    } finally {
      setBusy(false);
    }
  }

  async function updateProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError("");
    setBusy(true);
    const form = new FormData(event.currentTarget);
    const newPassword = String(form.get("new_password") || "");
    if (newPassword !== form.get("confirm_password")) {
      setError("Those passwords do not match.");
      setBusy(false);
      return;
    }
    try {
      const result = await readResponse<User & ErrorBody>(await secureFetch("/api/v1/auth/profile", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          email: form.get("email"),
          display_name: form.get("display_name"),
          new_password: newPassword || null,
        }),
      }));
      if (!result.response.ok) throw new Error(getError(result.data, "Could not update your profile."));
      setUser(result.data);
      setScreen("workspace");
    } catch (requestError) {
      setError(requestError instanceof Error ? requestError.message : "Could not update your profile.");
    } finally {
      setBusy(false);
    }
  }

  async function signOut() {
    setBusy(true);
    await secureFetch("/api/v1/auth/logout", { method: "POST" }).catch(() => undefined);
    setUser(null);
    setScreen("login");
    await primeCsrf();
    setBusy(false);
  }

  return (
    <main className="relative flex min-h-svh items-center justify-center overflow-hidden px-5 py-10 sm:px-8">
      <div aria-hidden="true" className="pointer-events-none absolute -left-36 -top-44 size-[440px] rounded-full bg-emerald-100/70 blur-3xl" />
      <div aria-hidden="true" className="pointer-events-none absolute -bottom-52 -right-40 size-[500px] rounded-full bg-lime-100/60 blur-3xl" />
      <div className="relative grid w-full max-w-6xl overflow-hidden rounded-[2rem] border border-white/80 bg-white/70 shadow-[0_24px_100px_-40px_rgba(25,57,43,0.28)] backdrop-blur-xl lg:min-h-[690px] lg:grid-cols-[1.05fr_0.95fr]">
        <section className="relative hidden flex-col justify-between overflow-hidden bg-[#173c31] p-12 text-white lg:flex xl:p-16">
          <div className="absolute inset-0 opacity-25" style={{ backgroundImage: "radial-gradient(#d2ead8 0.8px, transparent 0.8px)", backgroundSize: "20px 20px" }} />
          <div className="absolute -bottom-32 -left-28 size-96 rounded-full border border-emerald-100/20" />
          <div className="absolute -bottom-20 -left-16 size-72 rounded-full border border-emerald-100/20" />
          <div className="relative flex items-center gap-3"><BrandMark /><span className="font-semibold tracking-tight">treefime</span></div>
          <div className="relative max-w-md pb-8">
            <div className="mb-5 inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/10 px-3 py-1.5 text-xs font-medium text-emerald-50"><Sparkles size={14} />A little more clarity, every day</div>
            <h1 className="text-5xl font-semibold leading-[1.08] tracking-[-0.055em] xl:text-6xl">Work moves<br />better together.</h1>
            <p className="mt-6 max-w-sm text-[15px] leading-7 text-emerald-50/75">Bring projects, people, and the next right step into one calm workspace.</p>
            <div className="mt-11 flex items-center gap-3 text-sm text-emerald-50/80"><div className="flex -space-x-2"><span className="grid size-8 place-items-center rounded-full border-2 border-[#173c31] bg-amber-200 text-xs font-bold text-amber-900">J</span><span className="grid size-8 place-items-center rounded-full border-2 border-[#173c31] bg-sky-200 text-xs font-bold text-sky-900">M</span><span className="grid size-8 place-items-center rounded-full border-2 border-[#173c31] bg-rose-200 text-xs font-bold text-rose-900">A</span></div><span>Good work starts with a clear view.</span></div>
          </div>
          <p className="relative text-xs text-emerald-50/50">A thoughtful place to make progress.</p>
        </section>

        <section className="flex min-h-[650px] flex-col justify-center px-6 py-12 sm:px-12 lg:px-14 xl:px-20">
          <div className="mb-10 flex items-center gap-3 lg:hidden"><BrandMark /><span className="font-semibold tracking-tight">treefime</span></div>
          {screen === "loading" ? (
            <div className="mx-auto w-full max-w-sm animate-pulse"><div className="mb-4 h-7 w-48 rounded bg-muted" /><div className="mb-9 h-4 w-64 rounded bg-muted" /><div className="h-56 rounded-2xl bg-muted" /></div>
          ) : screen === "login" ? (
            <Card className="mx-auto w-full max-w-md border-0 bg-transparent shadow-none">
              <CardHeader className="px-0 pb-7">
                <div className="mb-2 grid size-11 place-items-center rounded-xl bg-secondary text-primary"><LockKeyhole size={20} /></div>
                <CardTitle className="text-3xl tracking-[-0.04em]">Welcome back</CardTitle>
                <CardDescription>Sign in to pick up where your team left off.</CardDescription>
              </CardHeader>
              <CardContent className="px-0">
                <form onSubmit={signIn} className="space-y-5">
                  <div className="space-y-2"><Label htmlFor="email">Email address</Label><Input id="email" name="email" type="email" autoComplete="username" placeholder="you@company.com" required maxLength={320} /></div>
                  <div className="space-y-2"><Label htmlFor="password">Password</Label><Input id="password" name="password" type="password" autoComplete="current-password" placeholder="Enter your password" required maxLength={128} /></div>
                  {error && <p role="alert" className="rounded-lg bg-red-50 px-3.5 py-3 text-sm text-red-700">{error}</p>}
                  <Button className="h-11 w-full" type="submit" disabled={busy}>{busy ? "Signing in…" : <>Sign in <ArrowRight size={16} /></>}</Button>
                </form>
                <div className="mt-7 flex items-start gap-2.5 rounded-xl border border-border/70 bg-white/65 p-3.5 text-xs leading-5 text-muted-foreground"><CircleHelp size={15} className="mt-0.5 shrink-0 text-primary" /><span>The first-run root credentials are printed in the API logs when the database is initialized.</span></div>
              </CardContent>
            </Card>
          ) : screen === "forced" ? (
            <Card className="mx-auto w-full max-w-md border-0 bg-transparent shadow-none">
              <CardHeader className="px-0 pb-7">
                <div className="mb-2 grid size-11 place-items-center rounded-xl bg-amber-100 text-amber-800"><ShieldCheck size={21} /></div>
                <CardTitle className="text-3xl tracking-[-0.04em]">Secure your account</CardTitle>
                <CardDescription>This first-run root account is limited until you replace its temporary email and password.</CardDescription>
              </CardHeader>
              <CardContent className="px-0">
                <form onSubmit={updateProfile} className="space-y-4">
                  <div className="space-y-2"><Label htmlFor="profile-email">New email address</Label><Input id="profile-email" name="email" type="email" autoComplete="email" placeholder="you@company.com" required maxLength={320} /></div>
                  <div className="space-y-2"><Label htmlFor="display-name">Your name</Label><Input id="display-name" name="display_name" autoComplete="name" defaultValue={user?.display_name || "System root"} required maxLength={80} /></div>
                  <div className="space-y-2"><Label htmlFor="new-password">New password</Label><Input id="new-password" name="new_password" type="password" autoComplete="new-password" placeholder="At least 12 characters" required minLength={12} maxLength={128} /></div>
                  <div className="space-y-2"><Label htmlFor="confirm-password">Confirm new password</Label><Input id="confirm-password" name="confirm_password" type="password" autoComplete="new-password" placeholder="Enter it once more" required minLength={12} maxLength={128} /></div>
                  {error && <p role="alert" className="rounded-lg bg-red-50 px-3.5 py-3 text-sm text-red-700">{error}</p>}
                  <Button className="h-11 w-full" type="submit" disabled={busy}>{busy ? "Saving securely…" : <>Save and continue <ArrowRight size={16} /></>}</Button>
                </form>
                <button onClick={signOut} className="mt-5 w-full text-center text-sm text-muted-foreground transition-colors hover:text-foreground" disabled={busy}>Sign out</button>
              </CardContent>
            </Card>
          ) : (
            <Card className="mx-auto w-full max-w-md border-0 bg-transparent shadow-none">
              <CardHeader className="px-0 pb-7">
                <div className="mb-2 grid size-11 place-items-center rounded-xl bg-emerald-100 text-emerald-800"><Check size={21} /></div>
                <CardTitle className="text-3xl tracking-[-0.04em]">You’re all set, {user?.display_name.split(" ")[0]}.</CardTitle>
                <CardDescription>Your account is secure and this workspace is ready for its first project.</CardDescription>
              </CardHeader>
              <CardContent className="space-y-5 px-0">
                <div className="rounded-xl border border-border/70 bg-white/70 p-4"><p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">Signed in as</p><p className="mt-1 font-medium">{user?.email}</p>{user?.is_root && <span className="mt-3 inline-flex rounded-full bg-secondary px-2.5 py-1 text-xs font-semibold text-secondary-foreground">System root</span>}</div>
                <div className="flex items-center gap-3 rounded-xl bg-white/50 p-4 text-sm text-muted-foreground"><div className="grid size-9 shrink-0 place-items-center rounded-lg bg-secondary text-primary"><Layers3 size={17} /></div><span>Your boards and projects will appear here as you create them.</span></div>
                <Button variant="outline" className="w-full" onClick={signOut} disabled={busy}><LogOut size={16} /> Sign out</Button>
              </CardContent>
            </Card>
          )}
          <div className="mt-9 flex items-center justify-center gap-2 text-xs text-muted-foreground"><ShieldCheck size={14} className="text-primary" />Protected with secure, rotating sessions</div>
        </section>
      </div>
    </main>
  );
}
