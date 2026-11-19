//! 规则包签名校验：ECDSA P-256 + SHA-256（CNG/BCrypt 直调，零新增 crate）。
//!
//! **选型取舍（R32-4b，用户拍板 D2）**：原定 Ed25519，但微软 CNG 官方算法标识符
//! 清单只有 `ECDSA_P256/384/521`（无 Ed25519/EdDSA 条目），故按用户允许的降级路径
//! 采用 ECDSA P-256 + SHA-256；验签仍走 CNG，不新增 crate、不引入重量运行时。
//!
//! 参数依据（2026-11-20 在本机 Win11 实测确认，见 LESSONS）：
//! - 公钥 blob = `BCRYPT_ECCKEY_BLOB { magic: 'ECS1'（ECDSA_PUBLIC_P256）, cbKey: 32 }`
//!   + `X[32]` + `Y[32]`（大端）；
//! - 验签调用 `BCryptVerifySignature(hKey, NULL, sha256(msg), 32, sig, 64, 0)`：
//!   ECDSA 的 `pPaddingInfo` 传 NULL、`dwFlags` 传 0（官方文档：flags=0 时
//!   padding 必须为 NULL），哈希算法由调用方预哈希决定；
//! - 签名必须是 IEEE P1363 裸 `r||s`（各 32 字节大端），不是 OpenSSL 默认的 DER；
//! - 返回 0 = 验签通过；`0xC000A000` = STATUS_INVALID_SIGNATURE。

/// ECDSA P-256 公钥（`X||Y`，各 32 字节大端）。
pub type P256PublicKey = [u8; 64];
/// IEEE P1363 签名（`r||s`，各 32 字节大端）。
pub type P256Signature = [u8; 64];

/// 校验 `signature` 是否为 `message` 在 `public_key` 下的合法 ECDSA P-256/SHA-256 签名。
pub fn verify_p256_sha256(
    public_key: &P256PublicKey,
    message: &[u8],
    signature: &P256Signature,
) -> bool {
    #[cfg(windows)]
    {
        verify_windows(public_key, message, signature)
    }
    #[cfg(not(windows))]
    {
        // 规则包更新通道仅 Windows（WinHTTP）；其他平台一律 fail-closed。
        let _ = (public_key, message, signature);
        false
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use sha2::{Digest, Sha256};

    use super::{P256PublicKey, P256Signature};

    /// `BCRYPT_ECDSA_PUBLIC_P256_MAGIC` 的字节序列为 `'E','C','S','1'`（小端 u32）。
    const ECDSA_PUBLIC_P256_MAGIC: u32 = 0x3153_4345;
    const KEY_BYTES: u32 = 32;

    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptOpenAlgorithmProvider(
            ph_algorithm: *mut *mut c_void,
            psz_alg_id: *const u16,
            psz_implementation: *const u16,
            dw_flags: u32,
        ) -> i32;
        fn BCryptImportKeyPair(
            h_algorithm: *mut c_void,
            h_import_key: *mut c_void,
            psz_blob_type: *const u16,
            ph_key: *mut *mut c_void,
            pb_input: *const u8,
            cb_input: u32,
            dw_flags: u32,
        ) -> i32;
        fn BCryptVerifySignature(
            h_key: *mut c_void,
            p_padding_info: *const c_void,
            pb_hash: *const u8,
            cb_hash: u32,
            pb_signature: *const u8,
            cb_signature: u32,
            dw_flags: u32,
        ) -> i32;
        fn BCryptDestroyKey(h_key: *mut c_void) -> i32;
        fn BCryptCloseAlgorithmProvider(h_algorithm: *mut c_void, dw_flags: u32) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 句柄 RAII：仅包裹成功取得的非空句柄，Drop 时按类型关闭。
    struct Handle(*mut c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            // 安全性：句柄来自 BCrypt 成功路径（非 NULL），且仅关闭一次。
            if !self.0.is_null() {
                unsafe { BCryptDestroyKey(self.0) };
            }
        }
    }

    struct AlgHandle(*mut c_void);

    impl Drop for AlgHandle {
        fn drop(&mut self) {
            // 安全性：句柄来自 BCryptOpenAlgorithmProvider 成功路径（非 NULL），仅关闭一次。
            if !self.0.is_null() {
                unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
            }
        }
    }

    pub(super) fn verify(
        public_key: &P256PublicKey,
        message: &[u8],
        signature: &P256Signature,
    ) -> bool {
        // 公钥 blob：8 字节头 + X[32] + Y[32]。
        let mut blob = [0u8; 72];
        blob[..4].copy_from_slice(&ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
        blob[4..8].copy_from_slice(&KEY_BYTES.to_le_bytes());
        blob[8..].copy_from_slice(public_key);

        let digest = Sha256::digest(message);
        let alg_id = wide("ECDSA_P256");
        let blob_type = wide("ECCPUBLICBLOB");

        // 安全性：以下调用参数均为有效缓冲区/宽字符串指针；句柄由 RAII 释放。
        unsafe {
            let mut alg: *mut c_void = std::ptr::null_mut();
            if BCryptOpenAlgorithmProvider(&mut alg, alg_id.as_ptr(), std::ptr::null(), 0) != 0
                || alg.is_null()
            {
                return false;
            }
            let alg = AlgHandle(alg);

            let mut key: *mut c_void = std::ptr::null_mut();
            let imported = BCryptImportKeyPair(
                alg.0,
                std::ptr::null_mut(),
                blob_type.as_ptr(),
                &mut key,
                blob.as_ptr(),
                blob.len() as u32,
                0,
            );
            if imported != 0 || key.is_null() {
                return false;
            }
            let key = Handle(key);

            BCryptVerifySignature(
                key.0,
                std::ptr::null(),
                digest.as_ptr(),
                digest.len() as u32,
                signature.as_ptr(),
                signature.len() as u32,
                0,
            ) == 0
        }
    }
}

#[cfg(windows)]
fn verify_windows(public_key: &P256PublicKey, message: &[u8], signature: &P256Signature) -> bool {
    imp::verify(public_key, message, signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试公钥（对应 `rulesKeyId="1"` 的私钥；私钥离线保存在仓库外）。
    const TEST_KEY: P256PublicKey = [
        0x4c, 0xf6, 0xdb, 0x68, 0x5a, 0xb6, 0x13, 0x27, 0xe5, 0xbe, 0xcd, 0x22, 0xb1, 0x55, 0x17,
        0x12, 0x2a, 0x0a, 0x26, 0xa3, 0x94, 0x54, 0x18, 0x93, 0x6b, 0x10, 0x26, 0xcc, 0x77, 0x5f,
        0x15, 0x67, 0x08, 0xf1, 0x56, 0x7f, 0xe8, 0x45, 0x54, 0xdf, 0x17, 0xa7, 0x23, 0x79, 0x13,
        0x23, 0x6c, 0xc2, 0x0d, 0xc8, 0x0e, 0x10, 0xd8, 0xbe, 0x67, 0xc6, 0xb2, 0x19, 0xaf, 0x50,
        0x4c, 0x3e, 0xe5, 0xd3,
    ];

    fn fixture_signature() -> P256Signature {
        let hex = "fe4f4ee0d8075981ea8cf72fb57965214f314f4cbdc49d2bb5eb965f16399cb0\
                   e1bf63e68b085ac1f8ea558a7a5f9105d3754bcb054c17e78fb2a494731d4082";
        let bytes = hex.replace([' ', '\n'], "");
        let mut out = [0u8; 64];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&bytes[i * 2..i * 2 + 2], 16).expect("夹具 hex");
        }
        out
    }

    #[cfg(windows)]
    #[test]
    fn verifies_real_signature_and_rejects_tampering() {
        let msg = b"pureslate-rules-test-1";
        let sig = fixture_signature();
        assert!(verify_p256_sha256(&TEST_KEY, msg, &sig), "合法签名应通过");

        let mut bad = sig;
        bad[0] ^= 0xff;
        assert!(!verify_p256_sha256(&TEST_KEY, msg, &bad), "篡改签名应失败");

        assert!(
            !verify_p256_sha256(&TEST_KEY, b"pureslate-rules-test-2", &sig),
            "换消息应失败"
        );
    }
}
