# Code signing the launcher and its installer (Azure Artifact Signing)

Why: an unsigned download makes Windows SmartScreen say "Windows protected your PC", and testers must click **More
info**, then **Run anyway**. A signed `AAReplaySetup.exe` and launcher show a verified publisher instead.

Microsoft renamed "Trusted Signing" to **Artifact Signing**. The portal and the docs now use the new name.
The tools are the same: SignTool with a "dlib" and a small `metadata.json`.

- Cost: the Basic plan is about **$9.99 a month** for up to 5,000 signatures with one certificate profile. A release
  uses three (the launcher, the setup and its uninstaller).
- The owner does all the steps in Part 1: the purchase, the sign-up and the identity check. A Claude session never
  enters payment details, account details or ID documents.

## Part 1: the owner, in the Azure portal (about an hour, then 1 to 30 business days of identity checking)

**Who can sign up.** Read this first:

- **Individuals:** only developers in the **United States or Canada** can sign up as an individual.
- **Organizations:** organizations in the **EU**, the UK and a few other countries can sign up. A registered Dutch
  *stichting* or a KvK-registered business counts as an organization.

The organization check asks for:

- the legal name and a website;
- **two email addresses on a domain the organization owns** (not gmail);
- a business identifier (the KvK number);
- the business address;
- the representative's own ID check.

It may also ask for documents, such as a KvK extract issued within the last 12 months. If the organization has no
own-domain email yet, set that up before you start. Sources: Microsoft's
[Artifact Signing quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart).

1. **An Azure subscription with billing.** Pay-as-you-go is fine. Free, trial and sponsored subscriptions cannot use
   Artifact Signing. Go to portal.azure.com, then Subscriptions, then Add.
2. **Turn the service on.** Go to the subscription, then **Resource providers**, find `Microsoft.CodeSigning`, and
   click **Register**.
3. **Make the signing account.** Search **Artifact Signing Accounts**, then Create:
   - Resource group: new, for example `aareplay-signing`.
   - Name: for example `aareplay`. The name must be unique in Azure.
   - Region: **West Europe**. Its endpoint is `https://weu.codesigning.azure.net`. Note it down.
   - Pricing tier: **Basic**.
4. **Let yourself do the identity check.** Open the account, then **Access control (IAM)**, Add role assignment,
   **Artifact Signing Identity Verifier**, and pick yourself. You also need at least Reader on the subscription,
   which the subscription's owner already has.
5. **Identity validation.** In the account, open **Identity validations**, then New identity, **Public**, then
   **Organization**. Fill in the fields listed above. The person named as representative then gets an ID check
   (government photo ID, through AU10TIX and Microsoft Authenticator). Wait for the status **Completed**. Microsoft
   says this takes 1 to 30 business days, and it may email asking for documents.
6. **Certificate profile.** In the account, open **Certificate profiles**, then Create, then **Public Trust**.
   - Name: for example `aareplay-public`.
   - Identity: the validated one.
   - Use **Public Trust**, not "Public Trust Test", which is only for trying things out.
7. **Allow signing.** On the certificate profile (or the account), open **Access control (IAM)**, then Add role
   assignment, then **Artifact Signing Certificate Profile Signer**. Pick the identity that will sign:
   - **Simplest: yourself.** Then on the PC that makes releases, install the Azure CLI
     (`winget install -e --id Microsoft.AzureCLI`) and run `az login` once in your own terminal. It opens your
     browser, and the sign-in stays on that PC. Run it again when a release says the sign-in ran out.
   - **For automation: an app registration.** Go to Microsoft Entra ID, then App registrations, then New. Give that
     app the Signer role and make a client secret. On the release PC, set `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and
     `AZURE_CLIENT_SECRET` as user environment variables. The secret is a password: keep it out of every repository
     and every chat.
8. **Tell the release session three values:** the account's endpoint (step 3), the account name and the certificate
   profile name. None of them is secret.

## Part 2: the release PC (a Claude session can do this once Part 1 is done)

1. Install the client tools: `winget install -e --id Microsoft.Azure.ArtifactSigningClientTools`. The other way is
   the NuGet package `Microsoft.ArtifactSigning.Client`, unpacked into the signing folder. They need the .NET 8
   runtime. SignTool comes with the Windows SDK; 10.0.22621 or newer is needed, and this PC has 10.0.26100.
2. Write `%LOCALAPPDATA%\AAEmuSandbox-dev\signing\metadata.json`. That folder is outside every repository, next to the
   update signing key's folder. `$env:AAREPLAY_SIGNING_DIR` can point elsewhere.

   ```json
   {
     "Endpoint": "https://weu.codesigning.azure.net",
     "CodeSigningAccountName": "aareplay",
     "CertificateProfileName": "aareplay-public"
   }
   ```

3. Check: `.\installer\sign.ps1 -Status` should say "signing is set up". Then sign a scratch copy of the exe:
   `.\installer\sign.ps1 -Files <copy>.exe -Require`. A 403 error usually means the endpoint's region does not match
   the account's region, or the Signer role is missing.

## Making a release

`.\installer\release.ps1 -Version 0.2.1 -KeyFile <update signing key file>` does every step:

1. It runs the tests and builds the launcher.
2. It signs the exe. This happens before the update manifest is made, because the manifest names the exe's SHA-256.
3. It makes and signs the update files in `release\`.
4. It builds `installer\out\AAReplaySetup.exe`. Inno Setup signs the setup and its uninstaller through `sign.ps1`.

When signing is not set up, it says so and makes an unsigned release, exactly as before. `-NoAuthenticode` makes an
unsigned one on purpose.

Checked 2026-11-22 with a throwaway self-signed test certificate (`-TestPfx`):

- the exe, the uninstaller and the setup were signed;
- without the setup, `-Require` stops the build;
- cargo's own cached exe stays unsigned.

The cargo cache needed a fix for that last point: the exe is a hard link to cargo's copy, so the script copies it
before signing. Not yet run against a real Artifact Signing account, since the account does not exist yet.

## After the first signed release

Keep the "More info, Run anyway" note on the join page, in `Tools/invites/README.md` and in the testers' guide until a
signed `AAReplaySetup.exe` has gone out and has been seen opening without the warning. A new certificate can still be
warned about for a short while, until SmartScreen has seen enough downloads. Then remove the note in all three places.
