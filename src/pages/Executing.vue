<script setup lang="ts">
import { computed } from "vue";
import { useCleanStore } from "../stores/clean";
import { ALL_DISPOSITIONS } from "../stores/cleanMetrics";
import { useCountUp } from "../composables/useCountUp";

const store = useCleanStore();

const DISP_CN: Record<string, string> = {
  direct: "直清",
  recycle: "回收站",
  quarantine: "隔离区",
};

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const gb = b / (1024 * 1024 * 1024);
  if (gb >= 1) return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb.toFixed(2)} GB`;
  const mb = b / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  return `${Math.round(b / 1024)} KB`;
}

// 总体进度百分比（分母 itemTotal，防 0）。
const percent = computed(() => {
  const t = store.itemTotal;
  if (t <= 0) return 0;
  return Math.min(100, Math.round((store.processed / t) * 100));
});

// 每条去向进度行：累计处理字节 + ok/fail/skip 计数。
const rows = computed(() =>
  ALL_DISPOSITIONS.map((d) => {
    const t = store.totals[d];
    return {
      key: d,
      label: DISP_CN[d],
      okBytes: t.okBytes,
      ok: t.okCount,
      fail: t.failCount,
      skip: t.skipCount,
    };
  }),
);

// 完成页首行（SAFETY §4.5：已释放 vs 已隔离，禁止混用"已清理"）。
const summary = computed(() => store.result);

// 完成态字节数补间（落定汇总，≤340ms；执行中高频数字不补间）。
const animatedFreed = useCountUp(() => store.freedBytes, { durationMs: 340 });
const animatedQuarantined = useCountUp(() => store.quarantinedBytes, { durationMs: 340 });

// 最近逐项（倒序，最新在顶）。
const recent = computed(() => [...store.progressEvents].reverse().slice(0, 6));
</script>

<template>
  <main class="exec">
    <header class="exec-head">
      <router-link to="/report" class="back">← 返回报告</router-link>
      <h1>清理执行</h1>
      <p class="sub">逐项处置：直清 / 回收站 / 隔离区</p>
    </header>

    <div v-if="store.error" class="error">执行失败：{{ store.error }}</div>

    <!-- 完成态 -->
    <template v-else-if="store.isDone && summary">
      <section class="done-card anim-rise d1">
        <div class="done-head">
          <div class="done-badge" aria-hidden="true">
            <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
              <path d="m5 12.5 4.4 4.4L19 7.4" />
            </svg>
          </div>
          <div class="done-headtext">
            <h2 class="done-title">清理完成</h2>
            <p class="sema-note">「已释放」= 直清 / 回收站已让渡体积；「已隔离」= 转入隔离区，尚未释放，保留期内可还原。</p>
          </div>
        </div>

        <div class="done-stats">
          <div class="dstat freed">
            <span class="dlabel">已释放</span>
            <span class="dnum g-num">{{ formatBytes(animatedFreed) }}</span>
            <span class="dnote">直清 / 回收站，体积已让渡</span>
          </div>
          <div class="dstat quar">
            <span class="dlabel">已隔离</span>
            <span class="dnum y-num">{{ formatBytes(animatedQuarantined) }}</span>
            <span class="dnote">尚未释放，可在隔离区一键还原</span>
          </div>
        </div>

        <div class="done-foot">
          <div class="counts">
            <span>完成 {{ summary.ok }}</span>
            <span class="fail" v-if="summary.fail > 0">失败 {{ summary.fail }}</span>
            <span class="skip" v-if="summary.skip > 0">跳过 {{ summary.skip }}</span>
          </div>
          <div class="actions">
            <router-link to="/log" class="btn">查看操作日志</router-link>
            <router-link to="/quarantine" class="btn ghost">去隔离区还原</router-link>
          </div>
        </div>
      </section>
    </template>

    <!-- 执行中 -->
    <section v-else class="card exec-card anim-rise d1">
      <div class="exec-stage">
        <div class="dial" :style="{ '--p': percent }">
          <div class="dial-core">
            <span class="dial-pct">{{ percent }}<small>%</small></span>
            <span class="dial-count">{{ store.processed }} / {{ store.itemTotal }} 项</span>
          </div>
        </div>

        <div class="exec-meta">
          <div class="exec-title">
            <span class="phase">{{ store.running ? "正在清理…" : "准备中…" }}</span>
            <span class="exec-sub">每步先写日志再执行</span>
          </div>
          <div class="bar">
            <div class="bar-fill" :style="{ transform: `scaleX(${percent / 100})` }"></div>
          </div>

          <!-- 分去向进度 -->
          <div class="disp-stats">
            <div v-for="row in rows" :key="row.key" class="stat">
              <div class="stat-head">
                <span class="stat-label">
                  <span class="d-icon" :class="row.key" aria-hidden="true">
                    <svg v-if="row.key === 'direct'" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round">
                      <circle cx="12" cy="12" r="9" />
                      <path d="m8.2 12.4 2.6 2.6 5-5.3" />
                    </svg>
                    <svg v-else-if="row.key === 'recycle'" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M4.5 7.5h15" />
                      <path d="M9.5 7.5V5.8c0-.7.6-1.3 1.3-1.3h2.4c.7 0 1.3.6 1.3 1.3v1.7" />
                      <path d="M6.6 7.5 7.5 19c.06.8.7 1.4 1.5 1.4h6c.8 0 1.44-.6 1.5-1.4l.9-11.5" />
                    </svg>
                    <svg v-else width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M12 3.5 19 6v5.2c0 4.6-3.1 7.6-7 9.3-3.9-1.7-7-4.7-7-9.3V6Z" />
                      <rect x="9.4" y="10.6" width="5.2" height="4.6" rx="1.1" />
                    </svg>
                  </span>
                  {{ row.label }}
                </span>
                <span class="stat-bytes">{{ formatBytes(row.okBytes) }}</span>
              </div>
              <div class="stat-counts">
                <span>ok {{ row.ok }}</span>
                <span class="f2" v-if="row.fail > 0">fail {{ row.fail }}</span>
                <span class="s2" v-if="row.skip > 0">skip {{ row.skip }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 逐项最近 -->
      <div class="recent" v-if="recent.length">
        <p class="m-label">最近处置</p>
        <ul class="recent-list">
          <li v-for="(e, i) in recent" :key="i" class="recent-row">
            <span class="st" :class="e.state">{{ { ok: "✓", fail: "✗", skip: "∅" }[e.state] }}</span>
            <span class="rp" :title="e.itemPath">{{ e.itemPath }}</span>
            <span class="rd">→ {{ DISP_CN[e.disposition] }}</span>
          </li>
        </ul>
      </div>

      <div class="actions">
        <button class="btn-cancel" :disabled="!store.running" @click="store.cancel()">取消清理</button>
        <p class="tip">取消后已完成的项不再回滚；隔离区项仍可还原。</p>
      </div>
    </section>
  </main>
</template>

<style scoped>
.exec {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  max-width: 1080px;
  margin: 0 auto;
}
.back {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-2);
  font-weight: 600;
  font-size: 0.82rem;
  padding: 4px 10px 4px 8px;
  margin-left: -8px;
  border-radius: var(--r-pill);
  transition:
    color var(--dur-fast) var(--ease-inout),
    background var(--dur-fast) var(--ease-inout);
}
.back:hover {
  color: var(--accent);
  background: rgba(37, 99, 235, 0.08);
}
.exec-head h1 {
  margin: 0.5rem 0 0.3rem;
  font-size: 1.6rem;
  font-weight: 720;
  letter-spacing: -0.02em;
}
.sub {
  margin: 0;
  color: var(--text-2);
  font-size: 0.9rem;
}
.error {
  color: var(--grade-red);
  background: rgba(255, 244, 242, 0.85);
  border: 1px solid rgba(214, 58, 47, 0.24);
  border-radius: var(--r-md);
  padding: 1rem 1.25rem;
  box-shadow: var(--shadow-1);
}
.card {
  background: var(--surface);
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  padding: 1.6rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.exec-stage {
  display: grid;
  grid-template-columns: 190px minmax(0, 1fr);
  gap: 1.7rem;
  align-items: center;
}
.dial {
  --p: 0;
  position: relative;
  width: 168px;
  height: 168px;
  margin: 0 auto;
  border-radius: 50%;
  background: conic-gradient(var(--accent) calc(var(--p) * 1%), rgba(19, 32, 54, 0.08) 0);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 14px 30px -22px rgba(37, 99, 235, 0.7);
  transition: background var(--dur-base) var(--ease-out);
}
.dial::before {
  content: "";
  position: absolute;
  inset: 12px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.96);
  box-shadow: inset 0 1px 0 #fff;
}
.dial-core {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.dial-pct {
  font-size: 2rem;
  font-weight: 740;
  letter-spacing: -0.03em;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.dial-pct small {
  font-size: 0.95rem;
  font-weight: 650;
  color: var(--text-2);
  margin-left: 1px;
}
.dial-count {
  font-size: 0.78rem;
  color: var(--text-2);
  font-variant-numeric: tabular-nums;
}
.exec-meta {
  display: flex;
  flex-direction: column;
  gap: 0.8rem;
  min-width: 0;
}
.exec-title {
  display: flex;
  align-items: baseline;
  gap: 0.6rem;
  flex-wrap: wrap;
}
.phase {
  font-size: 1.05rem;
  font-weight: 700;
}
.exec-sub {
  font-size: 0.78rem;
  color: var(--text-2);
}
.bar {
  height: 12px;
  border-radius: var(--r-pill);
  background: rgba(19, 32, 54, 0.07);
  box-shadow:
    inset 0 1px 2px rgba(16, 28, 52, 0.09),
    inset 0 0 0 1px rgba(19, 32, 54, 0.03);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  width: 100%;
  background: linear-gradient(90deg, #5b8cf5 0%, #2563eb 60%, #1d51d8 100%);
  border-radius: var(--r-pill);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.4);
  transform-origin: left center;
  transition: transform var(--dur-base) var(--ease-out);
}
.disp-stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.65rem;
}
.stat {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0.75rem 0.95rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  background: rgba(255, 255, 255, 0.55);
  transition: background var(--dur-fast) var(--ease-inout);
}
.stat:hover {
  background: rgba(255, 255, 255, 0.85);
}
.stat-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.stat-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.85rem;
  font-weight: 600;
}
.d-icon {
  width: 24px;
  height: 24px;
  flex: none;
  border-radius: 7px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.d-icon.direct {
  color: #12884a;
  background: rgba(18, 136, 74, 0.1);
}
.d-icon.recycle {
  color: #2563eb;
  background: rgba(37, 99, 235, 0.1);
}
.d-icon.quarantine {
  color: #b26a00;
  background: rgba(202, 158, 74, 0.14);
}
.stat-bytes {
  font-size: 1.05rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.stat-counts {
  font-size: 0.75rem;
  color: var(--text-2);
  display: flex;
  gap: 0.5rem;
}
.f2 {
  color: var(--grade-red);
}
.s2 {
  color: var(--text-2);
}
.m-label {
  margin: 0 0 0.4rem;
  font-size: 0.8rem;
  color: var(--text-2);
}
.recent-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.recent-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.8rem;
}
.st {
  width: 1.1rem;
  text-align: center;
  font-weight: 700;
}
.st.ok {
  color: var(--grade-green);
  animation: st-pop var(--dur-base) var(--ease-out);
}
.st.fail {
  color: var(--grade-red);
}
.st.skip {
  color: var(--text-2);
}
/* 逐项完成勾号弹现（≤base，不延迟安全操作反馈）*/
@keyframes st-pop {
  0% {
    opacity: 0;
    transform: scale(0.3);
  }
  60% {
    transform: scale(1.25);
  }
  100% {
    opacity: 1;
    transform: scale(1);
  }
}
.rp {
  flex: 1;
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rd {
  color: var(--accent);
  flex-shrink: 0;
}
.actions {
  display: flex;
  align-items: center;
  gap: 1rem;
}
.btn-cancel {
  background: linear-gradient(180deg, #c98800 0%, #b26a00 100%);
  color: #fff;
  border: none;
  padding: 0.62rem 1.3rem;
  border-radius: 11px;
  font-weight: 650;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 6px 16px -8px rgba(140, 84, 0, 0.65);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout),
    opacity var(--dur-fast) var(--ease-inout);
}
.btn-cancel:hover:not(:disabled) {
  filter: brightness(1.05);
}
.btn-cancel:active {
  transform: scale(0.98);
}
.btn-cancel:disabled {
  opacity: 0.45;
  box-shadow: none;
  cursor: not-allowed;
}
.tip {
  margin: 0;
  font-size: 0.8rem;
  color: var(--text-2);
}
.done-card {
  background:
    radial-gradient(520px 220px at 94% -30%, rgba(18, 161, 80, 0.12), rgba(18, 161, 80, 0) 70%),
    linear-gradient(135deg, rgba(255, 255, 255, 0.94), rgba(246, 251, 248, 0.8));
  border: 1px solid var(--glass-line);
  border-radius: var(--r-xl);
  padding: 1.9rem 2.1rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  backdrop-filter: var(--glass-blur-strong);
  -webkit-backdrop-filter: var(--glass-blur-strong);
  box-shadow:
    var(--glass-sheen),
    var(--shadow-2);
}
.done-head {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}
.done-badge {
  width: 46px;
  height: 46px;
  flex: none;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--grade-green);
  background: rgba(18, 136, 74, 0.1);
  box-shadow: inset 0 0 0 1px rgba(18, 136, 74, 0.18);
}
.done-headtext {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.done-title {
  margin: 0;
  font-size: 1.42rem;
  font-weight: 740;
  letter-spacing: -0.02em;
}
.done-stats {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.9rem;
}
.dstat {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 1rem 1.1rem;
  border-radius: var(--r-lg);
  border: 1px solid var(--glass-line);
}
.dstat.freed {
  background: linear-gradient(180deg, rgba(230, 246, 236, 0.85), rgba(255, 255, 255, 0.75));
  border-color: rgba(18, 136, 74, 0.2);
}
.dstat.quar {
  background: linear-gradient(180deg, rgba(253, 247, 235, 0.9), rgba(255, 255, 255, 0.75));
  border-color: rgba(202, 158, 74, 0.24);
}
.dlabel {
  font-size: 0.78rem;
  color: var(--text-2);
}
.dnum {
  font-size: 1.9rem;
  font-weight: 760;
  letter-spacing: -0.03em;
  line-height: 1.15;
}
.dnote {
  font-size: 0.76rem;
  color: var(--text-2);
}
.g-num {
  color: var(--grade-green);
  font-variant-numeric: tabular-nums;
}
.y-num {
  color: var(--grade-yellow);
  font-variant-numeric: tabular-nums;
}
.sema-note {
  margin: 0;
  font-size: 0.8rem;
  color: var(--text-2);
  line-height: 1.6;
}
.done-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  flex-wrap: wrap;
}
.counts {
  display: flex;
  gap: 0.75rem;
  font-size: 0.9rem;
  color: var(--text-2);
}
.counts .fail {
  color: var(--grade-red);
}
.counts .skip {
  color: var(--text-2);
}
.actions {
  display: flex;
  gap: 0.75rem;
  padding-top: 0.25rem;
}
.btn {
  display: inline-block;
  background: linear-gradient(180deg, #3b7bf5 0%, #2563eb 55%, #1d51d8 100%);
  color: #fff;
  border: none;
  padding: 0.62rem 1.3rem;
  border-radius: 11px;
  font-weight: 650;
  text-decoration: none;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.3),
    0 8px 20px -8px rgba(37, 99, 235, 0.55);
  transition:
    transform var(--dur-fast) var(--ease-out),
    filter var(--dur-fast) var(--ease-inout);
}
.btn:hover {
  filter: brightness(1.05);
}
.btn:active {
  transform: scale(0.98);
}
.btn.ghost {
  background: rgba(255, 255, 255, 0.75);
  color: var(--accent);
  border: 1px solid rgba(37, 99, 235, 0.4);
  box-shadow: none;
}
.btn.ghost:hover {
  background: #fff;
}
@media (max-width: 900px) {
  .exec-stage {
    grid-template-columns: minmax(0, 1fr);
  }
  .dial {
    width: 148px;
    height: 148px;
  }
}
@media (max-width: 680px) {
  .disp-stats {
    grid-template-columns: minmax(0, 1fr);
  }
  .done-stats {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>