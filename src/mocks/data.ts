/**
 * mock 假数据：覆盖全部 6 扫描维度（temp/large/dup/cache/startup/privacy），
 * 含中文路径、三档分级、重复文件组、中英文文件名。
 * 供报告页 / 扫描页在 VITE_MOCK=true 下联调，不依赖内核。
 */
import type {
  CategoryAggregate,
  FoundBytes,
  Grade,
  QuarantineEntry,
  ScanItem,
  ScanResult,
  StartupEntry,
} from "../types/ipc";

// ---- 内部工具 ----
function sha1ish(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  }
  return (h >>> 0).toString(16).padStart(16, "0");
}

type ItemSeed = Omit<ScanItem, "id"> & { idSource: string };

const MB = 1024 * 1024;

// ---- 六维度 seed（含中文路径 / 三档分级 / 重复组） ----
const seeds: ItemSeed[] = [
  // temp（🟢 direct）
  { idSource: "temp.user|C:\\Windows\\Temp\\installer.log", categoryId: "temp.user", label: "临时文件", path: "C:\\Windows\\Temp\\installer.log", sizeBytes: 4 * MB, grade: "green", disposition: "direct", reason: "系统安装残留临时文件，删除后自动重建", mtime: Date.now() - 3 * 86400e3 },
  { idSource: "temp.user|C:\\Users\\17599\\AppData\\Local\\Temp\\cache_微信.exe.mst", categoryId: "temp.user", label: "临时文件", path: "C:\\Users\\17599\\AppData\\Local\\Temp\\cache_微信.exe.mst", sizeBytes: 12 * MB, grade: "green", disposition: "direct", reason: "安装包临时缓存，可安全清理", mtime: Date.now() - 60e3 },
  // large（🟢 recycle）
  { idSource: "large.file|D:\\Downloads\\虚拟机镜像\\bigfile_amd64.iso", categoryId: "large.file", label: "大文件", path: "D:\\Downloads\\虚拟机镜像\\bigfile_amd64.iso", sizeBytes: 512 * MB, grade: "green", disposition: "recycle", reason: "超过 500MB 的大文件，建议先确认再删除（可入回收站）", mtime: Date.now() - 30 * 86400e3, atime: Date.now() - 180 * 86400e3 },
  // dup（🟡 quarantine，重复组）
  { idSource: "dup.file|C:\\Users\\17599\\Documents\\照片备份\\2023\\photo_001.jpg", categoryId: "dup.file", label: "重复文件", path: "C:\\Users\\17599\\Documents\\照片备份\\2023\\photo_001.jpg", sizeBytes: 88 * MB, grade: "yellow", disposition: "quarantine", reason: "与 photo_001_副本.jpg 内容相同，保留最早一份", mtime: Date.now() - 20 * 86400e3, dupGroup: "dupgrp-1" },
  { idSource: "dup.file|C:\\Users\\17599\\Downloads\\photo_001_副本.jpg", categoryId: "dup.file", label: "重复文件", path: "C:\\Users\\17599\\Downloads\\photo_001_副本.jpg", sizeBytes: 88 * MB, grade: "yellow", disposition: "quarantine", reason: "重复副本，可在隔离区保留 14 天后再清", mtime: Date.now() - 10 * 86400e3, dupGroup: "dupgrp-1" },
  // cache（🟡 quarantine，含 guard 提示）
  { idSource: "cache.wechat|C:\\Users\\17599\\Documents\\WeChat Files\\wx\\FileStorage\\Cache\\video_聊天.mp4", categoryId: "cache.wechat", label: "微信缓存", path: "C:\\Users\\17599\\Documents\\WeChat Files\\wx\\FileStorage\\Cache\\video_聊天.mp4", sizeBytes: 60 * MB, grade: "yellow", disposition: "quarantine", reason: "微信图片/视频缓存，清理后需重新登录，14 天内可还原", mtime: Date.now() - 2 * 86400e3 },
  { idSource: "cache.browser|C:\\Users\\17599\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data\\f_0001a2b3", categoryId: "cache.browser", label: "浏览器缓存", path: "C:\\Users\\17599\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data\\f_0001a2b3", sizeBytes: 3 * MB, grade: "yellow", disposition: "quarantine", reason: "浏览器访问缓存，清理后需重新加载页面图片", mtime: Date.now() - 1 * 86400e3 },
  // startup（🟡 启动项）
  { idSource: "startup.registry|HKCU:\\Run\\UpdateChecker", categoryId: "startup.registry", label: "启动项", path: "registry: HKCU\\...\\Run\\UpdateChecker", sizeBytes: 0, grade: "yellow", disposition: "quarantine", reason: "开机自启的更新检查程序，禁用后需重启生效", mtime: Date.now() - 90 * 86400e3 },
  // privacy（🔴 red，默认灰禁）
  { idSource: "privacy.history|C:\\Users\\17599\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\History", categoryId: "privacy.history", label: "浏览历史", path: "C:\\Users\\17599\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\History", sizeBytes: 2 * MB, grade: "red", disposition: "quarantine", reason: "浏览器访问历史，含私密信息，需专家模式+二次确认", mtime: Date.now() - 0.5 * 86400e3 },
  { idSource: "privacy.recent|C:\\Users\\17599\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\机密.公司财务报表.xlsx.lnk", categoryId: "privacy.recent", label: "最近文档", path: "C:\\Users\\17599\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\机密.公司财务报表.xlsx.lnk", sizeBytes: 1 * MB, grade: "red", disposition: "quarantine", reason: "最近访问文档记录，含敏感信息，需专家模式+二次确认", mtime: Date.now() - 3 * 86400e3 },
];

// ---- 组装 ScanItem（稳定 id） ----
function toItem(s: ItemSeed): ScanItem {
  const { idSource, ...rest } = s;
  return { id: sha1ish(idSource), ...rest };
}

// ---- 聚合计算 ----
function aggregate(items: ScanItem[]): CategoryAggregate[] {
  const map = new Map<string, CategoryAggregate>();
  for (const it of items) {
    const existing = map.get(it.categoryId);
    if (existing) {
      existing.totalBytes += it.sizeBytes;
      existing.itemCount += 1;
    } else {
      map.set(it.categoryId, {
        categoryId: it.categoryId,
        label: it.label,
        grade: it.grade,
        disposition: it.disposition,
        totalBytes: it.sizeBytes,
        itemCount: 1,
        reason: it.reason,
      });
    }
  }
  return [...map.values()];
}

function sumBy(items: ScanItem[], grade: Grade): number {
  return items.filter((i) => i.grade === grade).reduce((acc, i) => acc + i.sizeBytes, 0);
}

// ---- 主构造：ScanResult ----
export function buildMockScanResult(): ScanResult & { items: ScanItem[] } {
  const items = seeds.map(toItem);
  const totalBytes: FoundBytes = {
    green: sumBy(items, "green"),
    yellow: sumBy(items, "yellow"),
    red: sumBy(items, "red"),
  };
  return {
    scanId: "mock-scan-0001",
    startedAt: Date.now() - 5000,
    finishedAt: Date.now(),
    volume: "C:",
    aggregates: aggregate(items),
    itemCount: items.length,
    totalBytes,
    items,
  };
}

// ---- 隔离区 stub ----
export const QuarantineListStub: QuarantineEntry[] = [
  {
    id: "q-0001",
    originalPath: "C:\\Users\\17599\\Documents\\WeChat Files\\wx\\FileStorage\\Cache\\video_聊天.mp4",
    sizeBytes: 60 * MB,
    grade: "yellow",
    categoryId: "cache.wechat",
    movedAt: Date.now() - 2 * 86400e3,
    expiresAt: Date.now() + 12 * 86400e3,
    daysLeft: 12,
    state: "quarantined",
  },
  {
    id: "q-0002",
    originalPath: "C:\\Users\\17599\\Downloads\\photo_001_副本.jpg",
    sizeBytes: 88 * MB,
    grade: "yellow",
    categoryId: "dup.file",
    movedAt: Date.now() - 1 * 86400e3,
    expiresAt: Date.now() + 13 * 86400e3,
    daysLeft: 13,
    state: "quarantined",
  },
];

// ---- 启动项 stub ----
export const StartupListStub: StartupEntry[] = [
  { id: "st-0001", name: "UpdateChecker", publisher: "某软件公司", command: "C:\\Program Files\\UpdateChecker\\uc.exe --background", source: "hkcu_run", impact: "low", enabled: true },
  { id: "st-0002", name: "影音伴侣", publisher: "未知", command: "C:\\Users\\17599\\AppData\\Roaming\\MediaBuddy\\mb.exe", source: "hkcu_run", impact: "medium", enabled: true },
];