"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import Link from "next/link";
import { ArrowLeft, Copy, Layers3, LogOut, Plus, ShieldCheck, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type Workspace = { id: string; name: string; slug: string; description: string; archived: boolean; member_count: number };
type Member = { user_id: string; email: string; display_name: string; role: string };
type Invite = { id: string; email: string; role: string; token: string; expires_in_days: number };
type Body = { error?: string; workspace?: Workspace; owner_invitation?: Invite; token?: string; email?: string; role?: string; expires_in_days?: number };

export default function AdminPage() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [members, setMembers] = useState<Record<string, Member[]>>({});
  const [selected, setSelected] = useState("");
  const [csrf, setCsrf] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [inviteLink, setInviteLink] = useState("");
  const [busy, setBusy] = useState(false);

  const api = useCallback(async (path: string, init: RequestInit = {}) => {
    let token = csrf;
    if (!token) {
      const csrfResponse = await fetch("/api/v1/auth/csrf", { credentials: "include", cache: "no-store" });
      token = ((await csrfResponse.json()) as { csrf_token?: string }).csrf_token || "";
      setCsrf(token);
    }
    const headers = new Headers(init.headers);
    if (init.method && init.method !== "GET") headers.set("X-CSRF-Token", token);
    const response = await fetch(path, { ...init, headers, credentials: "include", cache: "no-store" });
    return { response, body: (await response.json().catch(() => ({}))) as Body };
  }, [csrf]);

  const load = useCallback(async () => {
    const me = await fetch("/api/v1/auth/me", { credentials: "include", cache: "no-store" });
    if (!me.ok) { window.location.assign("/"); return; }
    const user = (await me.json()) as { is_root?: boolean; must_update_credentials?: boolean };
    if (!user.is_root || user.must_update_credentials) { window.location.assign("/"); return; }
    const result = await api("/api/v1/workspaces");
    if (!result.response.ok) throw new Error(result.body.error || "Could not load workspaces.");
    setWorkspaces((await result.response.json()) as Workspace[]);
  }, [api]);

  useEffect(() => { void load().catch((e: unknown) => setError(e instanceof Error ? e.message : "Could not load admin panel.")); }, [load]);

  async function createWorkspace(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(""); setNotice(""); setInviteLink("");
    const formElement = event.currentTarget;
    const form = new FormData(event.currentTarget);
    try {
      const result = await api("/api/v1/workspaces", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ name: form.get("name"), description: form.get("description"), owner_email: form.get("owner_email") }) });
      if (!result.response.ok) throw new Error(result.body.error || "Could not create workspace.");
      const invitation = result.body.owner_invitation;
      if (invitation?.token) setInviteLink(`${window.location.origin}/invite/${invitation.token}`);
      setNotice("Workspace created. Copy and send the owner invitation link; it expires in 7 days.");
      formElement.reset();
      await load();
    } catch (e) { setError(e instanceof Error ? e.message : "Could not create workspace."); }
    finally { setBusy(false); }
  }

  async function loadMembers(workspaceId: string) {
    setSelected(workspaceId);
    const result = await api(`/api/v1/workspaces/${workspaceId}/members`);
    if (!result.response.ok) { setError(result.body.error || "Could not load members."); return; }
    const workspaceMembers = (await result.response.json()) as Member[];
    setMembers((current) => ({ ...current, [workspaceId]: workspaceMembers }));
  }

  async function invite(event: FormEvent<HTMLFormElement>, workspaceId: string) {
    event.preventDefault(); setBusy(true); setError(""); setNotice(""); setInviteLink("");
    const formElement = event.currentTarget;
    const form = new FormData(event.currentTarget);
    try {
      const result = await api(`/api/v1/workspaces/${workspaceId}/invitations`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email: form.get("email"), role: form.get("role") }) });
      if (!result.response.ok) throw new Error(result.body.error || "Could not create invitation.");
      if (result.body.token) setInviteLink(`${window.location.origin}/invite/${result.body.token}`);
      setNotice("Invitation created. Copy and send the link; it expires in 7 days.");
      formElement.reset();
      await loadMembers(workspaceId);
    } catch (e) { setError(e instanceof Error ? e.message : "Could not create invitation."); }
    finally { setBusy(false); }
  }

  return <main className="min-h-svh bg-muted/40 px-5 py-8 sm:px-8">
    <div className="mx-auto max-w-5xl space-y-7">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <div><Link href="/" className="inline-flex items-center gap-2 text-sm text-muted-foreground hover:text-foreground"><ArrowLeft size={15} /> Back to app</Link><div className="mt-4 flex items-center gap-3"><div className="grid size-10 place-items-center rounded-xl bg-primary text-primary-foreground"><Layers3 size={20} /></div><div><h1 className="text-2xl font-semibold tracking-tight">Workspace administration</h1><p className="text-sm text-muted-foreground">Manage workspace setup, owners, and invitations.</p></div></div></div>
        <Link href="/" className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-input bg-background px-4 text-sm font-semibold transition-colors hover:bg-muted"><LogOut size={15} /> Return to app</Link>
      </header>
      <div className="flex items-start gap-3 rounded-xl border border-emerald-900/10 bg-emerald-50/70 p-4 text-sm text-emerald-950"><ShieldCheck size={18} className="mt-0.5 shrink-0" /><p>Your platform role can manage workspace metadata and membership. It does not grant access to boards, cards, or workspace content.</p></div>
      <div className="grid gap-6 lg:grid-cols-[0.9fr_1.1fr]">
        <Card><CardHeader><CardTitle className="flex items-center gap-2"><Plus size={18} /> Create workspace</CardTitle><CardDescription>Every workspace starts with an invited owner. The platform account is not added as a member.</CardDescription></CardHeader><CardContent><form className="space-y-4" onSubmit={createWorkspace}><div className="space-y-2"><Label htmlFor="workspace-name">Workspace name</Label><Input id="workspace-name" name="name" required minLength={2} maxLength={100} placeholder="Product team" /></div><div className="space-y-2"><Label htmlFor="workspace-description">Description</Label><Input id="workspace-description" name="description" maxLength={500} placeholder="What this team works on" /></div><div className="space-y-2"><Label htmlFor="owner-email">Initial owner email</Label><Input id="owner-email" name="owner_email" type="email" required maxLength={320} placeholder="owner@example.com" /></div><Button className="w-full" disabled={busy}>{busy ? "Creating…" : "Create workspace and owner invite"}</Button></form></CardContent></Card>
        <Card><CardHeader><CardTitle className="flex items-center gap-2"><Users size={18} /> Workspaces</CardTitle><CardDescription>Metadata and membership counts only.</CardDescription></CardHeader><CardContent className="space-y-3">{workspaces.length === 0 ? <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">No workspaces yet.</p> : workspaces.map((workspace) => <div key={workspace.id} className="rounded-xl border bg-background p-4"><button className="flex w-full items-start justify-between gap-4 text-left" onClick={() => void loadMembers(workspace.id)}><span><span className="block font-medium">{workspace.name}</span><span className="mt-1 block text-xs text-muted-foreground">{workspace.slug} · {workspace.member_count} members{workspace.archived ? " · Archived" : ""}</span>{workspace.description && <span className="mt-2 block text-sm text-muted-foreground">{workspace.description}</span>}</span><span className="text-xs text-primary">{selected === workspace.id ? "Selected" : "Manage"}</span></button>{selected === workspace.id && <div className="mt-4 border-t pt-4"><p className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Members</p>{(members[workspace.id] || []).map((member) => <div key={member.user_id} className="flex justify-between py-1.5 text-sm"><span>{member.display_name} <span className="text-muted-foreground">{member.email}</span></span><span className="rounded-full bg-secondary px-2 py-0.5 text-xs">{member.role}</span></div>)}<form onSubmit={(event) => void invite(event, workspace.id)} className="mt-4 grid gap-2 sm:grid-cols-[1fr_130px_auto]"><Input name="email" type="email" required maxLength={320} placeholder="Invite by email" aria-label="Invite by email" /><select name="role" aria-label="Invitation role" className="h-10 rounded-md border border-input bg-background px-3 text-sm"><option value="member">Member</option><option value="admin">Admin</option><option value="owner">Owner</option></select><Button size="sm" disabled={busy}>Invite</Button></form></div>}</div>)}</CardContent></Card>
      </div>
      {(error || notice) && <p role="status" className={`rounded-lg px-4 py-3 text-sm ${error ? "bg-red-50 text-red-700" : "bg-emerald-50 text-emerald-800"}`}>{error || notice}</p>}
      {inviteLink && <Card><CardContent className="flex flex-wrap items-center justify-between gap-3 p-4"><div className="min-w-0"><p className="text-sm font-medium">Invitation link</p><p className="mt-1 break-all text-xs text-muted-foreground">{inviteLink}</p></div><Button variant="outline" onClick={() => void navigator.clipboard.writeText(inviteLink)}><Copy size={15} /> Copy link</Button></CardContent></Card>}
    </div>
  </main>;
}
