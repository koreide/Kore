import { useState, useCallback } from "react";
import {
  rbacListIdentities,
  rbacBuildMatrix,
  rbacCheckPermission,
  rbacReverseLookup,
  rbacAnalyzeForbidden,
  rbacListRoles,
  rbacNaturalLanguageQuery,
  type RbacIdentity,
  type RbacIdentityList,
  type PermissionMatrix,
  type ReverseLookupResult,
  type ForbiddenAnalysis,
  type RoleSummary,
  type NaturalLanguageRbacResult,
} from "@/lib/api";

export function useRbac(namespace: string | undefined) {
  const [identities, setIdentities] = useState<RbacIdentityList | null>(null);
  const [selectedIdentity, setSelectedIdentity] = useState<RbacIdentity | null>(null);
  const [matrix, setMatrix] = useState<PermissionMatrix | null>(null);
  const [roles, setRoles] = useState<RoleSummary[]>([]);
  const [reverseLookup, setReverseLookup] = useState<ReverseLookupResult | null>(null);
  const [forbiddenAnalysis, setForbiddenAnalysis] = useState<ForbiddenAnalysis | null>(null);
  const [nlResult, setNlResult] = useState<NaturalLanguageRbacResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchIdentities = useCallback(async () => {
    try {
      setError(null);
      const result = await rbacListIdentities();
      setIdentities(result);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const buildMatrix = useCallback(
    async (identity: RbacIdentity) => {
      try {
        setLoading(true);
        setError(null);
        const result = await rbacBuildMatrix(identity, namespace);
        setMatrix(result);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [namespace],
  );

  const checkPermission = useCallback(
    async (identity: RbacIdentity, verb: string, resource: string) => {
      try {
        return await rbacCheckPermission(identity, verb, resource, namespace);
      } catch (e) {
        setError(String(e));
        return null;
      }
    },
    [namespace],
  );

  const fetchRoles = useCallback(async () => {
    try {
      setError(null);
      const result = await rbacListRoles(namespace);
      setRoles(result);
    } catch (e) {
      setError(String(e));
    }
  }, [namespace]);

  const doReverseLookup = useCallback(
    async (roleKind: string, roleName: string, roleNamespace?: string) => {
      try {
        setLoading(true);
        setError(null);
        const result = await rbacReverseLookup(roleKind, roleName, roleNamespace);
        setReverseLookup(result);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const analyzeForbidden = useCallback(async (errorMessage: string) => {
    try {
      setLoading(true);
      setError(null);
      const result = await rbacAnalyzeForbidden(errorMessage);
      setForbiddenAnalysis(result);
      return result;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setLoading(false);
    }
  }, []);

  const naturalLanguageQuery = useCallback(
    async (query: string) => {
      try {
        setLoading(true);
        setError(null);
        const result = await rbacNaturalLanguageQuery(query, namespace);
        setNlResult(result);
        return result;
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setLoading(false);
      }
    },
    [namespace],
  );

  const refresh = useCallback(async () => {
    await fetchIdentities();
    if (selectedIdentity) {
      await buildMatrix(selectedIdentity);
    }
  }, [fetchIdentities, buildMatrix, selectedIdentity]);

  return {
    identities,
    selectedIdentity,
    setSelectedIdentity,
    matrix,
    roles,
    reverseLookup,
    forbiddenAnalysis,
    nlResult,
    loading,
    error,
    fetchIdentities,
    buildMatrix,
    checkPermission,
    fetchRoles,
    doReverseLookup,
    analyzeForbidden,
    naturalLanguageQuery,
    refresh,
  };
}
