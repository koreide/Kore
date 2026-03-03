import { useState, useMemo } from "react";
import { Search, User, Users, Box } from "lucide-react";
import type { RbacIdentity, RbacIdentityList } from "@/lib/api";
import { cn } from "@/lib/utils";

interface RbacIdentityPickerProps {
  identities: RbacIdentityList | null;
  selected: RbacIdentity | null;
  onSelect: (identity: RbacIdentity) => void;
  loading?: boolean;
}

function identityLabel(identity: RbacIdentity): string {
  switch (identity.kind) {
    case "ServiceAccount":
      return `${identity.namespace}/${identity.name}`;
    case "User":
    case "Group":
      return identity.name;
  }
}

function identityKey(identity: RbacIdentity): string {
  switch (identity.kind) {
    case "ServiceAccount":
      return `sa:${identity.namespace}/${identity.name}`;
    case "User":
      return `user:${identity.name}`;
    case "Group":
      return `group:${identity.name}`;
  }
}

export function RbacIdentityPicker({
  identities,
  selected,
  onSelect,
  loading,
}: RbacIdentityPickerProps) {
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState(false);

  const filtered = useMemo(() => {
    if (!identities) return { serviceAccounts: [], users: [], groups: [] };
    const q = search.toLowerCase();
    return {
      serviceAccounts: identities.service_accounts.filter(
        (sa) => sa.name.toLowerCase().includes(q) || sa.namespace.toLowerCase().includes(q),
      ),
      users: identities.users.filter((u) => u.toLowerCase().includes(q)),
      groups: identities.groups.filter((g) => g.toLowerCase().includes(q)),
    };
  }, [identities, search]);

  const selectedLabel = selected ? identityLabel(selected) : "Select identity...";

  return (
    <div className="relative">
      <button
        onClick={() => setOpen(!open)}
        className={cn(
          "flex items-center gap-2 px-3 py-1.5 rounded-md border text-sm font-mono",
          "bg-surface border-slate-700/50 hover:border-slate-600 transition-colors",
          "min-w-[200px] max-w-[320px]",
          selected ? "text-slate-200" : "text-slate-500",
        )}
      >
        {selected?.kind === "ServiceAccount" && (
          <Box className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
        )}
        {selected?.kind === "User" && <User className="w-3.5 h-3.5 text-blue-400 shrink-0" />}
        {selected?.kind === "Group" && <Users className="w-3.5 h-3.5 text-purple-400 shrink-0" />}
        <span className="truncate">{loading ? "Loading..." : selectedLabel}</span>
      </button>

      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute top-full left-0 mt-1 z-50 w-[320px] max-h-[360px] overflow-hidden rounded-lg glass border border-slate-700/50 shadow-xl">
            <div className="p-2 border-b border-slate-800">
              <div className="relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500" />
                <input
                  autoFocus
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder="Search identities..."
                  className="w-full pl-7 pr-3 py-1.5 bg-transparent text-sm text-slate-200 placeholder:text-slate-600 outline-none"
                />
              </div>
            </div>
            <div className="overflow-y-auto max-h-[300px]">
              {filtered.serviceAccounts.length > 0 && (
                <div className="p-1">
                  <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-slate-500 font-medium">
                    Service Accounts
                  </div>
                  {filtered.serviceAccounts.map((sa) => {
                    const id: RbacIdentity = {
                      kind: "ServiceAccount",
                      name: sa.name,
                      namespace: sa.namespace,
                    };
                    const key = identityKey(id);
                    const isSelected = selected && identityKey(selected) === key;
                    return (
                      <button
                        key={key}
                        onClick={() => {
                          onSelect(id);
                          setOpen(false);
                          setSearch("");
                        }}
                        className={cn(
                          "w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left",
                          "hover:bg-slate-700/40 transition-colors",
                          isSelected && "bg-slate-700/60",
                        )}
                      >
                        <Box className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                        <span className="font-mono text-slate-300 truncate">
                          <span className="text-slate-500">{sa.namespace}/</span>
                          {sa.name}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
              {filtered.users.length > 0 && (
                <div className="p-1">
                  <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-slate-500 font-medium">
                    Users
                  </div>
                  {filtered.users.map((name) => {
                    const id: RbacIdentity = { kind: "User", name };
                    const key = identityKey(id);
                    const isSelected = selected && identityKey(selected) === key;
                    return (
                      <button
                        key={key}
                        onClick={() => {
                          onSelect(id);
                          setOpen(false);
                          setSearch("");
                        }}
                        className={cn(
                          "w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left",
                          "hover:bg-slate-700/40 transition-colors",
                          isSelected && "bg-slate-700/60",
                        )}
                      >
                        <User className="w-3.5 h-3.5 text-blue-400 shrink-0" />
                        <span className="font-mono text-slate-300 truncate">{name}</span>
                      </button>
                    );
                  })}
                </div>
              )}
              {filtered.groups.length > 0 && (
                <div className="p-1">
                  <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-slate-500 font-medium">
                    Groups
                  </div>
                  {filtered.groups.map((name) => {
                    const id: RbacIdentity = { kind: "Group", name };
                    const key = identityKey(id);
                    const isSelected = selected && identityKey(selected) === key;
                    return (
                      <button
                        key={key}
                        onClick={() => {
                          onSelect(id);
                          setOpen(false);
                          setSearch("");
                        }}
                        className={cn(
                          "w-full flex items-center gap-2 px-2 py-1.5 rounded text-sm text-left",
                          "hover:bg-slate-700/40 transition-colors",
                          isSelected && "bg-slate-700/60",
                        )}
                      >
                        <Users className="w-3.5 h-3.5 text-purple-400 shrink-0" />
                        <span className="font-mono text-slate-300 truncate">{name}</span>
                      </button>
                    );
                  })}
                </div>
              )}
              {!filtered.serviceAccounts.length &&
                !filtered.users.length &&
                !filtered.groups.length && (
                  <div className="p-4 text-center text-sm text-slate-500">No identities found</div>
                )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
