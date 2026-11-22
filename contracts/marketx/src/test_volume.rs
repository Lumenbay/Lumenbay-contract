//! Volume Discount Tests for the Lumenbay escrow contract
//!
//! Tests for verifying volume-based fee discount functionality
//!
//! ## Test Coverage
//!
//! 1. Volume updates after escrow release
//! 2. Tier calculation based on volume (real XLM-sized thresholds)
//! 3. Whitelist overrides volume discount
//! 4. Default tiers set on initialize
//! 5. Multiple releases accumulate volume
//! 6. Fee amounts: first-time buyer pays the full fee, tiers take a share
//!    *of the fee* off, and prior volume can never cancel the fee (regression)
//! 7. `set_volume_tiers` validation, auth and reset-window preservation

mod volume_tests {
    use crate::{Contract, ContractClient, VolumeTierConfig, STROOPS_PER_TOKEN};
    use soroban_sdk::{
        testutils::Address as _,
        token::{Client as TokenClient, StellarAssetClient},
        Address, Bytes, Env,
    };

    const FEE_BPS: u32 = 500; // 5%
    const MIN_FEE: i128 = 0;
    const MAX_FEE: i128 = 0;

    /// 1 XLM in stroops.
    const XLM: i128 = STROOPS_PER_TOKEN;

    struct Ctx<'a> {
        env: Env,
        admin: Address,
        buyer: Address,
        seller: Address,
        token: TokenClient<'a>,
        token_id: Address,
        client: ContractClient<'a>,
        nonce: u32,
    }

    impl Ctx<'_> {
        /// Create, fund and release one escrow; returns the fee charged.
        fn trade(&mut self, amount: i128) -> i128 {
            self.nonce += 1;
            let meta = Bytes::from_slice(&self.env, &self.nonce.to_be_bytes());
            let seller_before = self.token.balance(&self.seller);
            let id = self.client.create_escrow(
                &self.buyer,
                &self.seller,
                &self.token_id,
                &amount,
                &Some(meta),
                &None,
                &None,
                &None,
            );
            self.client.fund_escrow(&id);
            self.client.release_escrow(&id);
            let paid_to_seller = self.token.balance(&self.seller) - seller_before;
            amount - paid_to_seller
        }
    }

    fn setup_with_fee<'a>(fee_bps: u32) -> Ctx<'a> {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        let token = TokenClient::new(&env, &sac.address());
        let token_admin = StellarAssetClient::new(&env, &sac.address());
        let token_id = sac.address();

        let contract_id = env.register(Contract, ());
        let client = ContractClient::new(&env, &contract_id);

        client.initialize(&admin, &admin, &fee_bps, &MIN_FEE, &MAX_FEE);
        token_admin.mint(&buyer, &(1_000_000 * XLM));
        token.approve(&buyer, &contract_id, &i128::MAX, &1000);

        Ctx {
            env,
            admin,
            buyer,
            seller,
            token,
            token_id,
            client,
            nonce: 0,
        }
    }

    fn setup<'a>() -> Ctx<'a> {
        setup_with_fee(FEE_BPS)
    }

    #[test]
    fn test_volume_updated_after_escrow_release() {
        let mut ctx = setup();
        ctx.trade(100_000);
        assert_eq!(ctx.client.get_buyer_volume(&ctx.buyer), 100_000);
    }

    #[test]
    fn test_tier_calculation_from_volume() {
        let mut ctx = setup();

        // 0.01 XLM of volume used to reach tier 1; it must not any more.
        ctx.trade(100_000);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 0);

        // 600 + 600 XLM = 1,200 XLM crosses the 1,000 XLM tier-1 threshold.
        ctx.trade(600 * XLM);
        ctx.trade(600 * XLM);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 1);
    }

    #[test]
    fn test_whitelist_prevents_fee() {
        let mut ctx = setup();
        ctx.client.add_fee_whitelist(&ctx.buyer);
        assert_eq!(ctx.trade(1_000_000), 0);
    }

    #[test]
    fn test_default_tiers_set_on_initialize() {
        let ctx = setup();
        let tiers = ctx.client.get_volume_tiers();
        assert_eq!(tiers.tier_1_threshold, 1_000 * XLM);
        assert_eq!(tiers.tier_2_threshold, 10_000 * XLM);
        assert_eq!(tiers.tier_3_threshold, 100_000 * XLM);
        assert_eq!(tiers.tier_1_discount_bps, 100);
        assert_eq!(tiers.tier_2_discount_bps, 250);
        assert_eq!(tiers.tier_3_discount_bps, 500);
    }

    #[test]
    fn test_volume_accumulates() {
        let mut ctx = setup();
        ctx.trade(100_000);
        ctx.trade(50_000);
        assert_eq!(ctx.client.get_buyer_volume(&ctx.buyer), 150_000);
    }

    #[test]
    fn test_high_volume_tier_3() {
        let mut ctx = setup();
        ctx.trade(100_000 * XLM);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 3);
    }

    // ── Fee amounts (1% platform fee, as deployed) ──────────────────────────

    #[test]
    fn test_first_time_buyer_pays_one_percent() {
        let mut ctx = setup_with_fee(100);
        let fee = ctx.trade(XLM);
        assert_eq!(fee, XLM / 100); // 0.01 XLM = 100,000 stroops
        assert_eq!(ctx.token.balance(&ctx.seller), XLM - XLM / 100);
        assert_eq!(
            ctx.client.get_pending_fee(&ctx.admin, &ctx.token_id),
            XLM / 100
        );
    }

    /// Regression: a 1% fee minus a 1% (100 bps) tier discount used to be 0%,
    /// floored to 1 stroop, for any buyer with >= 0.01 XLM of prior volume.
    #[test]
    fn test_prior_volume_still_pays_one_percent() {
        let mut ctx = setup_with_fee(100);
        ctx.trade(XLM);
        ctx.trade(5 * XLM);
        assert_eq!(ctx.trade(XLM), XLM / 100);
    }

    #[test]
    fn test_tier_buyers_get_discount_off_the_fee() {
        let mut ctx = setup_with_fee(100);

        // First 1,000 XLM is priced at tier 0: the full 1%.
        assert_eq!(ctx.trade(1_000 * XLM), 10 * XLM);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 1);

        // Tier 1: 1% off the fee. 100 XLM -> 1 XLM fee -> 0.99 XLM.
        assert_eq!(ctx.trade(100 * XLM), XLM - XLM / 100);

        // Reach tier 2 (10,000 XLM), then 2.5% off: 1 XLM -> 0.975 XLM.
        ctx.trade(9_000 * XLM);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 2);
        assert_eq!(ctx.trade(100 * XLM), XLM - XLM / 40);

        // Reach tier 3 (100,000 XLM), then 5% off: 1 XLM -> 0.95 XLM.
        ctx.trade(90_000 * XLM);
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 3);
        assert_eq!(ctx.trade(100 * XLM), XLM - XLM / 20);
    }

    /// Instances initialized by older builds hold the old, stroop-sized tiers
    /// in storage. After upgrading, the fee must already be sane (at most 5%
    /// off the fee) before the admin corrects the tiers with `set_volume_tiers`.
    #[test]
    fn test_legacy_stored_tiers_cannot_cancel_fee() {
        let mut ctx = setup_with_fee(100);
        ctx.client
            .set_volume_tiers(&100_000, &1_000_000, &10_000_000, &100, &250, &500);

        ctx.trade(XLM); // volume now above the legacy tier-3 threshold
        assert_eq!(ctx.client.get_buyer_tier(&ctx.buyer), 3);
        assert_eq!(ctx.trade(XLM), 95_000); // 1% minus 5% of it, not 1 stroop
    }

    #[test]
    fn test_discount_is_capped_at_half_the_fee() {
        let mut ctx = setup_with_fee(100);
        ctx.client
            .set_volume_tiers(&1, &2, &3, &5_000, &5_000, &5_000);
        ctx.trade(XLM);
        assert_eq!(ctx.trade(XLM), XLM / 200);
    }

    // ── set_volume_tiers ────────────────────────────────────────────────────

    #[test]
    fn test_set_volume_tiers_updates_config_and_keeps_reset_ledger() {
        let ctx = setup();
        let before = ctx.client.get_volume_tiers();

        ctx.client
            .set_volume_tiers(&(500 * XLM), &(5_000 * XLM), &(50_000 * XLM), &0, &0, &0);

        assert_eq!(
            ctx.client.get_volume_tiers(),
            VolumeTierConfig {
                tier_1_threshold: 500 * XLM,
                tier_2_threshold: 5_000 * XLM,
                tier_3_threshold: 50_000 * XLM,
                tier_1_discount_bps: 0,
                tier_2_discount_bps: 0,
                tier_3_discount_bps: 0,
                reset_ledger: before.reset_ledger,
            }
        );
    }

    #[test]
    fn test_zero_discounts_disable_volume_pricing() {
        let mut ctx = setup_with_fee(100);
        ctx.client.set_volume_tiers(&1, &2, &3, &0, &0, &0);
        ctx.trade(XLM);
        assert_eq!(ctx.trade(XLM), XLM / 100);
    }

    #[test]
    fn test_set_volume_tiers_rejects_invalid_config() {
        let ctx = setup();
        let c = &ctx.client;
        // thresholds must be positive and strictly increasing
        assert!(c.try_set_volume_tiers(&0, &2, &3, &0, &0, &0).is_err());
        assert!(c.try_set_volume_tiers(&2, &2, &3, &0, &0, &0).is_err());
        assert!(c.try_set_volume_tiers(&1, &3, &2, &0, &0, &0).is_err());
        // discounts must be non-decreasing and within the cap
        assert!(c
            .try_set_volume_tiers(&1, &2, &3, &300, &200, &400)
            .is_err());
        assert!(c.try_set_volume_tiers(&1, &2, &3, &0, &0, &5_001).is_err());
        // config unchanged after rejections
        assert_eq!(c.get_volume_tiers().tier_1_threshold, 1_000 * XLM);
    }

    #[test]
    fn test_set_volume_tiers_requires_admin() {
        let ctx = setup();
        ctx.env.set_auths(&[]);
        assert!(ctx
            .client
            .try_set_volume_tiers(&1, &2, &3, &0, &0, &0)
            .is_err());
    }
}
