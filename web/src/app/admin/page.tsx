"use client";

import { useEffect, useState, useCallback } from "react";
import Link from "next/link";
import { useAuth } from "@/lib/auth";

interface AdminUser {
  id: string;
  email: string;
  name?: string;
  avatar_url?: string;
  role: string;
  approved: boolean;
  created_at: string;
}

interface Invite {
  id: string;
  email: string;
  invited_by: string;
  created_at: string;
  accepted_at?: string;
}

export default function AdminPage() {
  const { user, loading: authLoading } = useAuth();
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [loading, setLoading] = useState(true);
  const [inviteEmail, setInviteEmail] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    try {
      const [usersRes, invitesRes] = await Promise.all([
        fetch("/api/admin/users", { credentials: "include" }),
        fetch("/api/admin/invites", { credentials: "include" }),
      ]);
      if (usersRes.ok) {
        const data = await usersRes.json();
        setUsers(data.users || []);
      }
      if (invitesRes.ok) {
        const data = await invitesRes.json();
        setInvites(data.invites || []);
      }
    } catch (e) {
      console.error("Failed to fetch admin data", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!authLoading && user?.role === "admin") {
      fetchData();
    }
  }, [authLoading, user, fetchData]);

  const approveUser = async (id: string) => {
    await fetch(`/api/admin/users/${id}`, {
      method: "PATCH",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ approved: true }),
    });
    setMessage("User approved");
    fetchData();
  };

  const deleteUser = async (id: string) => {
    if (!confirm("Are you sure you want to remove this user?")) return;
    await fetch(`/api/admin/users/${id}`, {
      method: "DELETE",
      credentials: "include",
    });
    setMessage("User removed");
    fetchData();
  };

  const toggleRole = async (id: string, currentRole: string) => {
    const newRole = currentRole === "admin" ? "member" : "admin";
    await fetch(`/api/admin/users/${id}`, {
      method: "PATCH",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ role: newRole }),
    });
    setMessage(`Role changed to ${newRole}`);
    fetchData();
  };

  const sendInvite = async () => {
    if (!inviteEmail.trim()) return;
    const res = await fetch("/api/admin/invites", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email: inviteEmail.trim().toLowerCase() }),
    });
    if (res.ok) {
      setMessage(`Invite sent to ${inviteEmail}`);
      setInviteEmail("");
      fetchData();
    } else {
      const data = await res.json().catch(() => ({}));
      setMessage(`Error: ${data.error || "Failed to send invite"}`);
    }
  };

  if (authLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center text-zinc-400">
        Loading...
      </div>
    );
  }

  if (!user || user.role !== "admin") {
    return (
      <div className="min-h-screen flex flex-col items-center justify-center gap-4">
        <p className="text-zinc-500 text-lg">🔒 Admin access required</p>
        <Link href="/" className="text-blue-600 hover:underline">
          ← Back to home
        </Link>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-4xl mx-auto px-4 py-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Link href="/" className="text-zinc-400 hover:text-zinc-600 transition-colors">
              ← Back
            </Link>
            <h1 className="text-xl font-bold text-zinc-900 dark:text-zinc-100">
              ⚙️ Admin Panel
            </h1>
          </div>
        </div>
      </header>

      <main className="max-w-4xl mx-auto px-4 py-8 space-y-8">
        {message && (
          <div className="p-3 rounded-lg bg-green-50 dark:bg-green-950 text-green-700 dark:text-green-400 text-sm border border-green-200 dark:border-green-800">
            {message}
            <button
              onClick={() => setMessage(null)}
              className="ml-3 text-green-500 hover:text-green-700"
            >
              ✕
            </button>
          </div>
        )}

        {/* Invite Section */}
        <section>
          <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">
            Invite User
          </h2>
          <div className="flex gap-2">
            <input
              type="email"
              placeholder="user@example.com"
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && sendInvite()}
              className="flex-1 px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 text-sm focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <button
              onClick={sendInvite}
              className="px-4 py-2 text-sm rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors"
            >
              Send Invite
            </button>
          </div>
        </section>

        {/* Users Section */}
        <section>
          <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">
            Users ({users.length})
          </h2>
          {loading ? (
            <p className="text-zinc-400">Loading...</p>
          ) : (
            <div className="space-y-2">
              {users.map((u) => (
                <div
                  key={u.id}
                  className="flex items-center gap-3 p-3 rounded-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800"
                >
                  {u.avatar_url ? (
                    <img
                      src={u.avatar_url}
                      alt={u.name || u.email}
                      className="w-10 h-10 rounded-full"
                    />
                  ) : (
                    <div className="w-10 h-10 rounded-full bg-zinc-200 dark:bg-zinc-700 flex items-center justify-center text-zinc-500 font-medium">
                      {(u.name || u.email)[0].toUpperCase()}
                    </div>
                  )}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-zinc-900 dark:text-zinc-100 truncate">
                        {u.name || u.email}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 text-xs rounded-full ${
                          u.role === "admin"
                            ? "bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400"
                            : "bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400"
                        }`}
                      >
                        {u.role}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 text-xs rounded-full ${
                          u.approved
                            ? "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                            : "bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400"
                        }`}
                      >
                        {u.approved ? "approved" : "pending"}
                      </span>
                    </div>
                    <p className="text-sm text-zinc-500 truncate">{u.email}</p>
                  </div>
                  <div className="flex gap-1 shrink-0">
                    {!u.approved && (
                      <button
                        onClick={() => approveUser(u.id)}
                        className="px-2.5 py-1 text-xs rounded-md bg-green-600 hover:bg-green-700 text-white transition-colors"
                      >
                        Approve
                      </button>
                    )}
                    <button
                      onClick={() => toggleRole(u.id, u.role)}
                      className="px-2.5 py-1 text-xs rounded-md bg-zinc-200 dark:bg-zinc-700 hover:bg-zinc-300 dark:hover:bg-zinc-600 text-zinc-700 dark:text-zinc-300 transition-colors"
                      title={`Switch to ${u.role === "admin" ? "member" : "admin"}`}
                    >
                      {u.role === "admin" ? "↓ Member" : "↑ Admin"}
                    </button>
                    <button
                      onClick={() => deleteUser(u.id)}
                      className="px-2.5 py-1 text-xs rounded-md bg-red-100 dark:bg-red-900/30 hover:bg-red-200 dark:hover:bg-red-900/50 text-red-700 dark:text-red-400 transition-colors"
                    >
                      Remove
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Invites Section */}
        {invites.length > 0 && (
          <section>
            <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">
              Pending Invites
            </h2>
            <div className="space-y-2">
              {invites.map((inv) => (
                <div
                  key={inv.id}
                  className="flex items-center gap-3 p-3 rounded-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800"
                >
                  <div className="flex-1">
                    <span className="text-zinc-900 dark:text-zinc-100">{inv.email}</span>
                    <span className="text-xs text-zinc-400 ml-2">
                      invited {new Date(inv.created_at).toLocaleDateString()}
                    </span>
                  </div>
                  {inv.accepted_at && (
                    <span className="text-xs text-green-600 dark:text-green-400">✓ accepted</span>
                  )}
                </div>
              ))}
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
