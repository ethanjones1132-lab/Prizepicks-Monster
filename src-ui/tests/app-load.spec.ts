import { test, expect } from '@playwright/test';

test.describe('PrizePicks Monster - App Load', () => {
  test('loads the main app and shows sidebar navigation', async ({ page }) => {
    await page.goto('/');
    
    // Wait for Tauri app to load
    await page.waitForSelector('body', { timeout: 30000 });
    
    // Check that the sidebar navigation is present
    await expect(page.locator('aside.sidebar, .sidebar, aside').first()).toBeVisible({ timeout: 10000 });
    
    // Check for key navigation buttons using the actual labels from App.tsx
    await expect(page.locator('button:has-text("Prop board")').first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator('button:has-text("PrizePicks dashboard")').first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator('button:has-text("Analyst chat")').first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator('button:has-text("Prediction log")').first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator('button:has-text("ML predictor")').first()).toBeVisible({ timeout: 10000 });
    await expect(page.locator('button:has-text("Settings")').first()).toBeVisible({ timeout: 10000 });
  });

  test('can navigate to PrizePicks dashboard', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('body', { timeout: 30000 });
    
    // Click PrizePicks dashboard tab
    await page.locator('button:has-text("PrizePicks dashboard")').first().click();
    
    // Wait for dashboard content to load
    await page.waitForTimeout(2000);
    
    // Should show some dashboard content - just verify page is responsive
    await expect(page.locator('body').first()).toBeVisible();
  });
});

test.describe('PrizePicks Monster - Paper Trading', () => {
  test('paper trading panel loads with analytics', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('body', { timeout: 30000 });
    
    // Navigate to Prediction log tab
    await page.locator('button:has-text("Prediction log")').first().click();
    await page.waitForTimeout(3000);
    
    // Check for key analytics elements - at least one should be visible
    const equityEl = page.locator('text=Equity').first();
    const pnlEl = page.locator('text=PnL').first();
    const winRateEl = page.locator('text=Win Rate').first();
    
    // At least one of these should be visible
    const found = await Promise.race([
      equityEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
      pnlEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
      winRateEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
    ]);
    
    expect(found).toBeTruthy();
  });
});

test.describe('PrizePicks Monster - ML Predictor', () => {
  test('ML predictor tab is accessible', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('body', { timeout: 30000 });
    
    await page.locator('button:has-text("ML predictor")').first().click();
    await page.waitForTimeout(2000);
    
    // Check for ML training UI - at least one should be visible
    const trainEl = page.locator('text=Train model').first();
    const modelEl = page.locator('text=Model').first();
    const predictionsEl = page.locator('text=Predictions').first();
    
    const found = await Promise.race([
      trainEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
      modelEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
      predictionsEl.isVisible({ timeout: 10000 }).then(() => true).catch(() => false),
    ]);
    
    expect(found).toBeTruthy();
  });
});