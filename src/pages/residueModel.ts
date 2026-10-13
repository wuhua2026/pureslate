/**
 * R27-3 卸载残留 UI 纯函数模型。
 * 去向规则（SPEC §6.7 / SAFETY §2.2 R27-3 条款）：
 * - 每组二选一：移入隔离区（默认，14 天可还原）/ 直接删除（永久，🔴 专家令牌）；
 * - 不提供回收站档；同一批可混用（如快捷方式直删、安装目录隔离）。
 */
import type {
  Disposition,
  ResidueCandidate,
  ResidueKind,
  ResidueSelectionParams,
} from "../types/ipc";

/** 来源徽章文案。 */
export function kindLabel(kind: ResidueKind): string {
  switch (kind) {
    case "install_dir":
      return "安装目录";
    case "start_menu":
      return "开始菜单";
    case "app_data":
      return "AppData";
  }
}

/** 单项去向选择（candidateId → 去向；缺省 quarantine）。 */
export type DispositionChoice = "quarantine" | "direct";

export interface ResidueRowState {
  checked: boolean;
  disposition: DispositionChoice;
}

/** 初始行状态：默认勾选 + 隔离区（可还原）。 */
export function initialRowState(candidates: ResidueCandidate[]): Map<string, ResidueRowState> {
  const m = new Map<string, ResidueRowState>();
  for (const c of candidates) {
    m.set(c.id, { checked: true, disposition: "quarantine" });
  }
  return m;
}

/** 汇总选中项（与字节）。 */
export interface ResidueSummary {
  selected: number;
  total: number;
  selectedBytes: number;
  /** 选中里选了"直接删除"的组数。 */
  directCount: number;
  quarantineCount: number;
  /** 是否有选中组选择永久删除（决定是否需要专家令牌）。 */
  needsToken: boolean;
}

export function summarize(
  candidates: ResidueCandidate[],
  state: Map<string, ResidueRowState>,
): ResidueSummary {
  let selected = 0;
  let selectedBytes = 0;
  let directCount = 0;
  for (const c of candidates) {
    const s = state.get(c.id);
    if (!s?.checked) continue;
    selected += 1;
    selectedBytes += c.sizeBytes;
    if (s.disposition === "direct") directCount += 1;
  }
  return {
    selected,
    total: candidates.length,
    selectedBytes,
    directCount,
    quarantineCount: selected - directCount,
    needsToken: directCount > 0,
  };
}

/** 把行状态序列化为后端入参（仅勾选项）。 */
export function buildSelections(
  state: Map<string, ResidueRowState>,
): ResidueSelectionParams[] {
  const out: ResidueSelectionParams[] = [];
  for (const [candidateId, s] of state) {
    if (!s.checked) continue;
    out.push({
      candidateId,
      disposition: (s.disposition === "direct" ? "direct" : "quarantine") as Disposition,
    });
  }
  return out;
}

/** 切换"全选/全不选"（不改变已选去向）。 */
export function toggleAll(
  state: Map<string, ResidueRowState>,
  candidates: ResidueCandidate[],
  checked: boolean,
): Map<string, ResidueRowState> {
  const next = new Map(state);
  for (const c of candidates) {
    const cur = next.get(c.id) ?? { checked: true, disposition: "quarantine" as const };
    next.set(c.id, { ...cur, checked });
  }
  return next;
}

/** 路径末段展示（根路径过长时给个可识别短名）。 */
export function rootTail(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
