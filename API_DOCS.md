# API Documentation

Base URLs:

- Local: `http://localhost:4000`
- Production example: `https://api.ecommerce.steak.brotherstaste.dev`

**Standard Error Response:**

```json
{
  "error": {
    "status": 400,
    "message": "Validation error message"
  }
}
```

**Auth / Cookie / CORS Notes:**

- ทุก endpoint ที่ต้อง auth ใช้ `Authorization: Bearer <access_token>`
- `refresh_token` จะถูกส่งผ่าน `Set-Cookie` แบบ `HttpOnly` เท่านั้น และ **ไม่อยู่ใน response body**
- refresh cookie ใช้ `Path=/api/auth` และ `SameSite=Strict`
- ถ้าเรียกจาก browser และต้องการให้ cookie ทำงาน ต้องใช้ `credentials: "include"` ใน `fetch` หรือ `withCredentials: true` ใน Axios โดยเฉพาะ `verify-email`, `login`, `google/login`, `github/login`, `google/link/start`, `github/link/start`, `oauth/exchange`, `refresh`, `logout`, `PUT /api/users/me/password`, และ `POST /api/users/me/set-password`
- ถ้าเรียกจาก browser frontend, origin ของ frontend ต้องตรงกับค่า `APP_URL` ที่ backend ใช้อยู่
- ถ้า deploy หลาย environment ควรแยก base URL และ frontend origin ให้ตรงกันในแต่ละ environment
- backend ใช้ refresh token rotation แบบ token family และรองรับ concurrent refresh ซ้ำ token เดิมแบบ idempotent ภายในหน้าต่างสั้น ๆ ประมาณ 5 วินาทีเท่านั้น
- frontend ยังควรทำ `single-flight refresh` เอง และ retry original request ได้แค่ 1 รอบหลัง refresh สำเร็จ

**Security & Role Validation:**

- ระบบ Backend จะทำการตรวจสอบ **Role** และ **Account Status (is_active)** จาก Database โดยตรงทุกครั้งที่มีการเรียกใช้ request (Real-time Verification)
- แม้ว่าใน JWT จะมี Claim `role` อยู่ แต่ Backend จะยึดข้อมูลจาก Database เป็นหลักเสมอเพื่อความปลอดภัยสูงสุด
- หาก User ถูกระงับการใช้งาน (Banned) หรือถูกเปลี่ยน Role (เช่น จาก Admin เป็น User) จะมีผลทันทีใน request ถัดไป โดยไม่ต้องรอให้ Token หมดอายุ
- Frontend สามารถใช้ข้อมูลจาก `GET /api/users/me` เป็นหลักในการตัดสินใจแสดงผล UI ตามสิทธิ์ (Role) และสถานะการตั้งรหัสผ่าน (`has_password`) ของผู้ใช้

**Recommended Frontend Auth Flow:**

1. สมัครด้วย `POST /api/auth/register`
2. พา user ไปหน้า OTP และเก็บ `email` ที่ใช้สมัครไว้
3. ยืนยันด้วย `POST /api/auth/verify-email`
4. เก็บ `access_token` จาก response body และปล่อยให้ browser เก็บ `refresh_token` cookie เอง
5. เวลา reload หน้า หรือเปิดแอปใหม่ ให้เรียก `POST /api/auth/refresh` เพื่อขอ access token ใหม่
6. ถ้า `POST /api/auth/login` ตอบ `403 Email not verified` ให้พา user กลับไปหน้า OTP และเปิดปุ่ม resend
7. ถ้ามีหลาย request พร้อมกัน ให้มี refresh promise/shared lock แค่ 1 ตัวต่อ browser context และให้ request อื่นรอผลนั้นแทน

**Recommended Frontend Login Flow:**

1. ยิง `POST /api/auth/login`
2. ถ้าได้ `200` ถือว่า login สำเร็จ
3. ถ้าได้ `403 Email not verified` แปลว่า email/password คู่นี้ถูกต้องแล้ว แต่บัญชียังไม่ verify
4. พา user ไปหน้า OTP และเรียก `POST /api/auth/verify-email`
5. ถ้า OTP ถูก endpoint นี้จะ login ให้ทันทีและคืน `access_token` พร้อม `refresh_token` cookie
6. เรียก `POST /api/auth/resend-verification` เฉพาะตอน OTP หมดอายุหรือ user ขอ code ใหม่

**Recommended Frontend Password Recovery Flow:**

1. หน้า `Forgot password` ยิง `POST /api/auth/forgot-password`
2. ไม่ว่ามี email นี้ในระบบหรือไม่ backend จะตอบ `200` แบบ generic ถ้าไม่ติด cooldown/rate limit
3. ถ้า user ได้ code ทางอีเมล ให้พาไปหน้า `Reset password`
4. หน้า reset ยิง `POST /api/auth/reset-password`
5. reset สำเร็จแล้วให้พาไปหน้า login และล้าง access token ฝั่ง frontend

**Recommended Frontend Social Link Flow:**

1. ให้ user login ด้วยวิธีเดิมก่อน
2. จากหน้า settings/security เรียก `POST /api/auth/google/link/start` หรือ `POST /api/auth/github/link/start`
3. ส่ง `Authorization: Bearer <access_token>` และ `credentials: "include"` เพื่อให้ browser เก็บ OAuth state cookies
4. เอา `authorize_url` จาก response แล้วพา browser ไป URL นั้น
5. หลัง provider callback สำเร็จ backend จะ redirect กลับ `APP_URL{redirect_to}?link_provider=...&link_status=success`
6. ถ้า link ไม่สำเร็จ backend จะ redirect กลับ `APP_URL{redirect_to}?link_provider=...&link_error=...`
7. flow นี้ไม่ออก token ใหม่ เพราะใช้ session ปัจจุบันของ user ต่อได้เลย

---

## Auth

### POST `/api/auth/register`

สร้าง user แบบยังไม่ยืนยันอีเมล (`is_verified = false`) และส่ง verification code ไปทาง email

**Request Body:**

```json
{
  "name": "Jane Doe",
  "email": "user@example.com",
  "password": "12345678"
}
```

**หมายเหตุ:** ถ้าผู้ใช้ยังไม่ verify ภายใน 7 วัน บัญชีที่ยังไม่ยืนยันอาจถูก cleanup อัตโนมัติ

ถ้า email นี้มีบัญชีที่ยังไม่ verify อยู่แล้ว ระบบจะอัปเดตข้อมูล pending account เดิม (`name`, `image`, `password`) และส่ง OTP ใหม่ให้ ไม่ได้สร้าง user ซ้ำ

**Response 200:**

```json
{
  "message": "Verification code sent to your email"
}
```

---

### POST `/api/auth/verify-email`

ยืนยัน email ด้วย code 6 หลัก → เปิดใช้งานบัญชีและรับ tokens ได้ทันที

**Request Body:**

```json
{
  "email": "user@example.com",
  "code": "123456"
}
```

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@example.com",
    "image": null,
    "role": "USER"
  }
}
```

_(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)_

**หมายเหตุ:**

- OTP มีอายุ 15 นาที
- ถ้ามีการกด resend ให้ถือว่า code เก่าควรใช้ไม่ได้แล้ว และ frontend ควรบอก user ให้ใช้ code ล่าสุดเท่านั้น

---

### POST `/api/auth/resend-verification`

ส่ง verification code ใหม่ให้บัญชีที่ยังไม่ verify

**Request Body:**

```json
{
  "email": "user@example.com"
}
```

**Response 200:**

```json
{
  "message": "Verification code resent to your email"
}
```

**หมายเหตุ:**

- ใช้ได้เฉพาะบัญชีที่ยังไม่ verify
- OTP ใหม่มีอายุ 15 นาที
- เมื่อ resend สำเร็จ frontend ควร reset countdown และบอก user ให้ใช้ code ล่าสุดเท่านั้น

---

### POST `/api/auth/login`

Login ด้วย email + password → return access token ใน body และ refresh token ใน cookie

**Request Body:**

```json
{
  "email": "user@example.com",
  "password": "12345678"
}
```

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@example.com",
    "image": null,
    "role": "USER"
  }
}
```

**หมายเหตุ:** `refresh_token` จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly

**ถ้ายังไม่ verify email:** backend จะตรวจ password ก่อน ถ้า credential ถูกแต่บัญชียังไม่ verify จะตอบ `403` พร้อมข้อความ `Email not verified`

---

### POST `/api/auth/forgot-password`

ขอรหัสรีเซ็ตรหัสผ่านผ่านอีเมล

**Request Body:**

```json
{
  "email": "user@example.com"
}
```

**Response 200:**

```json
{
  "message": "If this email can be reset, a verification code has been sent."
}
```

**หมายเหตุ:**

- endpoint นี้ตอบ `200` แบบ generic เสมอเพื่อลด email enumeration
- ถ้าบัญชีนี้มีสิทธิ์ reset password ระบบจะส่ง code 6 หลักที่มีอายุ 15 นาที
- ใช้ได้กับบัญชีที่ active และ verify แล้วเท่านั้น
- มี email cooldown 60 วินาทีต่อ email เพื่อกันการกดส่งรหัสซ้ำถี่เกินไป

---

### POST `/api/auth/reset-password`

รีเซ็ตรหัสผ่านด้วย code 6 หลักที่ส่งทางอีเมล

**Request Body:**

```json
{
  "email": "user@example.com",
  "code": "123456",
  "new_password": "new-password-123"
}
```

**Response 200:**

```json
{
  "message": "Password reset successfully. Please log in again."
}
```

**หมายเหตุ:**

- `new_password` ต้องมีอย่างน้อย 8 ตัวอักษร และไม่เกิน 128 ตัวอักษร
- code มีอายุ 15 นาทีและใช้ได้ครั้งเดียว
- reset สำเร็จแล้ว backend จะ revoke `refresh_token` ทุกอันของ user
- backend จะไม่ auto-login หลัง reset password

### Common Auth Errors

| Endpoint                             | Status | Message                                                                           | Frontend Action                                  |
| ------------------------------------ | ------ | --------------------------------------------------------------------------------- | ------------------------------------------------ |
| `POST /api/auth/register`            | `409`  | `Email already registered`                                                        | พาไปหน้า login หรือแจ้งว่าบัญชีนี้ถูกใช้งานแล้ว  |
| `POST /api/auth/verify-email`        | `400`  | `No pending verification found for this email`                                    | พากลับ flow สมัครหรือให้กรอก email ใหม่          |
| `POST /api/auth/verify-email`        | `400`  | `Verification code has expired`                                                   | เปิดปุ่ม resend และให้ user ขอ code ใหม่         |
| `POST /api/auth/verify-email`        | `400`  | `Email already verified`                                                          | พาไปหน้า login                                   |
| `POST /api/auth/verify-email`        | `429`  | `Too many verification attempts. Please request a new code.`                      | ล็อกฟอร์มชั่วคราวและบังคับให้กด resend           |
| `POST /api/auth/resend-verification` | `400`  | `Email already verified`                                                          | พาไปหน้า login                                   |
| `POST /api/auth/resend-verification` | `400`  | `No pending verification found for this email`                                    | พากลับ flow สมัคร                                |
| `POST /api/auth/login`               | `403`  | `Email not verified`                                                              | พาไปหน้า OTP พร้อม email เดิม แล้วให้กรอก OTP ต่อได้ทันที |
| `POST /api/auth/forgot-password`     | `429`  | `Please wait 60 seconds before requesting another password reset code.` / `Too many forgot_password attempts. Please try again later.` | ปิดปุ่มส่ง code ชั่วคราวและแสดง countdown 60 วินาที |
| `POST /api/auth/reset-password`      | `400`  | `No pending verification found for this email` / `Verification code has expired` / `Invalid verification code` | ให้ user ขอ code ใหม่หรือลองกรอกใหม่            |
| `POST /api/auth/reset-password`      | `429`  | `Too many verification attempts. Please request a new code.`                      | ล็อกฟอร์มชั่วคราวและบังคับให้กดขอ code ใหม่      |
| `POST /api/auth/google/login`        | `409`  | `An account with this email already exists. Please sign in with your existing method to continue.` | ให้ user login ด้วยวิธีเดิมก่อน ไม่ควรสร้าง social session ใหม่ |
| `POST /api/auth/github/login`        | `409`  | `An account with this email already exists. Please sign in with your existing method to continue.` | ให้ user login ด้วยวิธีเดิมก่อน ไม่ควรสร้าง social session ใหม่ |
| `GET /api/auth/google/link/callback` | redirect | `link_error=oauth_account_already_linked` / `oauth_provider_conflict` / `google_link_failed` | อยู่หน้า settings เดิมแล้วแสดงข้อความว่า link ไม่สำเร็จ |
| `GET /api/auth/github/link/callback` | redirect | `link_error=oauth_account_already_linked` / `oauth_provider_conflict` / `github_link_failed` | อยู่หน้า settings เดิมแล้วแสดงข้อความว่า link ไม่สำเร็จ |
| `POST /api/auth/oauth/exchange`      | `401`  | `OAuth login ticket is invalid or expired` / `OAuth login ticket has expired`     | เริ่ม social login ใหม่จากปุ่ม provider เดิม     |
| `POST /api/auth/refresh`             | `401`  | `No refresh token cookie` / `Invalid refresh token` / `Refresh token has expired` / `Refresh session has been revoked. Please login again.` / `Refresh token reuse detected. Please login again.` | ล้าง state auth ฝั่ง frontend แล้วพาไปหน้า login |

---

### GET `/api/auth/google/start`

เริ่ม Google OAuth จาก backend โดยส่ง browser ไปยัง Google

**Query Params:**

- `exchange_url`: URL callback ของ frontend ที่ backend จะ redirect กลับหลัง login สำเร็จ
- `redirect_to`: path ปลายทางหลัง frontend exchange ticket เสร็จ เช่น `/account`

**ข้อกำหนดสำคัญ:**

- `exchange_url` ต้องใช้ origin เดียวกับ `APP_URL` ของ backend
- `exchange_url` ต้องมี path เป็น `/login/oauth/callback` เท่านั้น
- `redirect_to` ต้องเป็น relative path ที่ขึ้นต้นด้วย `/`; ถ้าไม่ส่งหรือส่งไม่ถูกต้อง ระบบจะ fallback เป็น `/account`
- Google OAuth client ควร whitelist callback URLs ของ backend อย่างน้อย:
  - `{API_BASE_URL}/api/auth/google/callback`
  - `{API_BASE_URL}/api/auth/google/link/callback`

**Flow:**

1. Frontend พา browser มายิง endpoint นี้
2. Backend สร้าง `state` cookie, `nonce` cookie, และ PKCE `code_verifier` cookie แล้ว redirect ไป Google
3. Google redirect กลับ `GET /api/auth/google/callback`
4. Backend สร้าง one-time login ticket แล้ว redirect กลับ `exchange_url?ticket=...&redirectTo=...`

**ถ้า OAuth ไม่สำเร็จ:** backend จะ redirect ไปที่ `APP_URL/login?oauth_error=...`

`oauth_error` ที่ควรรองรับจาก flow นี้ เช่น:

- `invalid_oauth_state`
- `missing_oauth_callback`
- `missing_oauth_code`
- `missing_oauth_pkce_verifier`
- `google_access_denied`
- `google_sign_in_failed`
- `oauth_account_conflict`
- `oauth_ticket_failed`

---

### GET `/api/auth/google/callback`

Google redirect callback ของ backend

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

---

### POST `/api/auth/google/link/start`

เริ่ม Google OAuth สำหรับ link บัญชี Google เข้ากับ user ที่ login อยู่แล้ว

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "redirect_to": "/account/security"
}
```

**Response 200:**

```json
{
  "authorize_url": "https://accounts.google.com/o/oauth2/v2/auth?client_id=..."
}
```

**Flow:**

1. Frontend เรียก endpoint นี้ด้วย `Authorization` header และ `credentials: "include"`
2. Backend สร้าง `state`, `nonce`, PKCE `code_verifier`, และ signed link-session cookie
3. Frontend redirect browser ไป `authorize_url`
4. Google redirect กลับ `GET /api/auth/google/link/callback`
5. Backend link provider ให้ user ปัจจุบัน แล้ว redirect กลับ `APP_URL{redirect_to}`

**หมายเหตุ:**

- route นี้ไม่ออก token ใหม่
- ถ้า user ผูก Google เดิมไว้อยู่แล้วและเป็น account เดิม endpoint callback จะถือว่าสำเร็จแบบ idempotent

---

### GET `/api/auth/google/link/callback`

Google callback สำหรับ social account linking

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

**Redirect success ตัวอย่าง:**

```text
APP_URL/account/security?link_provider=google&link_status=success
```

**Redirect error ตัวอย่าง:**

```text
APP_URL/account/security?link_provider=google&link_error=oauth_account_already_linked
```

`link_error` ที่ควรรองรับจาก flow นี้ เช่น:

- `invalid_oauth_state`
- `invalid_oauth_link_session`
- `missing_oauth_code`
- `missing_oauth_pkce_verifier`
- `google_link_access_denied`
- `oauth_account_already_linked`
- `oauth_provider_conflict`
- `google_link_failed`
- `account_suspended`
- `link_session_expired`

---

### POST `/api/auth/google/login`

Low-level endpoint สำหรับ login ด้วย Google ID Token โดยตรง → สร้าง user อัตโนมัติ (ถ้ายังไม่มี) + return tokens

เหมาะกับ BFF หรือระบบภายในที่มี `id_token` อยู่แล้ว แต่ flow production ที่แนะนำคือ `GET /api/auth/google/start`

**Request Body:**

```json
{
  "token": "eyJhbGciOiJSUzI1NiIsImtpZCI..."
}
```

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@gmail.com",
    "image": "https://lh3.googleusercontent.com/...",
    "role": "USER"
  }
}
```

_(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)_

**Common errors:**

- `409 An account with this email already exists. Please sign in with your existing method to continue.`
- `401 Google email is not verified`
- `401 Invalid ID Token nonce`

---

### GET `/api/auth/github/start`

เริ่ม GitHub OAuth จาก backend โดยส่ง browser ไปยัง GitHub

**Query Params:**

- `exchange_url`: URL callback ของ frontend ที่ backend จะ redirect กลับหลัง login สำเร็จ
- `redirect_to`: path ปลายทางหลัง frontend exchange ticket เสร็จ เช่น `/account`

**ข้อกำหนดสำคัญ:**

- `exchange_url` ต้องใช้ origin เดียวกับ `APP_URL` ของ backend
- `exchange_url` ต้องมี path เป็น `/login/oauth/callback` เท่านั้น
- `redirect_to` ต้องเป็น relative path ที่ขึ้นต้นด้วย `/`; ถ้าไม่ส่งหรือส่งไม่ถูกต้อง ระบบจะ fallback เป็น `/account`
- GitHub OAuth app ควร whitelist callback URLs ของ backend อย่างน้อย:
  - `{API_BASE_URL}/api/auth/github/callback`
  - `{API_BASE_URL}/api/auth/github/link/callback`

**Flow:**

1. Frontend พา browser มายิง endpoint นี้
2. Backend สร้าง `state` cookie แล้ว redirect ไป GitHub
3. GitHub redirect กลับ `GET /api/auth/github/callback`
4. Backend สร้าง one-time login ticket แล้ว redirect กลับ `exchange_url?ticket=...&redirectTo=...`

**ถ้า OAuth ไม่สำเร็จ:** backend จะ redirect ไปที่ `APP_URL/login?oauth_error=...`

`oauth_error` ที่ควรรองรับจาก flow นี้ เช่น:

- `invalid_oauth_state`
- `missing_oauth_callback`
- `missing_oauth_code`
- `github_access_denied`
- `github_sign_in_failed`
- `oauth_account_conflict`
- `oauth_ticket_failed`

---

### GET `/api/auth/github/callback`

GitHub redirect callback ของ backend

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

---

### POST `/api/auth/github/link/start`

เริ่ม GitHub OAuth สำหรับ link บัญชี GitHub เข้ากับ user ที่ login อยู่แล้ว

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "redirect_to": "/account/security"
}
```

**Response 200:**

```json
{
  "authorize_url": "https://github.com/login/oauth/authorize?client_id=..."
}
```

**Flow:**

1. Frontend เรียก endpoint นี้ด้วย `Authorization` header และ `credentials: "include"`
2. Backend สร้าง `state` และ signed link-session cookie
3. Frontend redirect browser ไป `authorize_url`
4. GitHub redirect กลับ `GET /api/auth/github/link/callback`
5. Backend link provider ให้ user ปัจจุบัน แล้ว redirect กลับ `APP_URL{redirect_to}`

**หมายเหตุ:**

- route นี้ไม่ออก token ใหม่
- ถ้า user ผูก GitHub เดิมไว้อยู่แล้วและเป็น account เดิม endpoint callback จะถือว่าสำเร็จแบบ idempotent

---

### GET `/api/auth/github/link/callback`

GitHub callback สำหรับ social account linking

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

**Redirect success ตัวอย่าง:**

```text
APP_URL/account/security?link_provider=github&link_status=success
```

**Redirect error ตัวอย่าง:**

```text
APP_URL/account/security?link_provider=github&link_error=oauth_account_already_linked
```

`link_error` ที่ควรรองรับจาก flow นี้ เช่น:

- `invalid_oauth_state`
- `invalid_oauth_link_session`
- `missing_oauth_code`
- `github_link_access_denied`
- `oauth_account_already_linked`
- `oauth_provider_conflict`
- `github_link_failed`
- `account_suspended`
- `link_session_expired`

---

### POST `/api/auth/github/login`

Low-level endpoint สำหรับ login ด้วย GitHub Access Code → สร้าง user อัตโนมัติ (ถ้ายังไม่มี) + return tokens

เหมาะกับ BFF หรือระบบภายในที่มี `code` อยู่แล้ว แต่ flow production ที่แนะนำคือ `GET /api/auth/github/start`

**Request Body:**

```json
{
  "code": "a4b5c6d7e8f9g0h1i2j3..."
}
```

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@github.com",
    "image": "https://avatars.githubusercontent.com/u/123456?v=4",
    "role": "USER"
  }
}
```

_(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)_

**Common errors:**

- `409 An account with this email already exists. Please sign in with your existing method to continue.`
- `401 No primary, verified email found on GitHub`

---

### POST `/api/auth/oauth/exchange`

แลก one-time social login ticket ที่ backend ออกให้ใน callback กลับมาเป็น access token + refresh token

ใช้โดย frontend callback route หลังจาก backend social OAuth สำเร็จแล้ว

**Request Body:**

```json
{
  "ticket": "0195dd2a-9b4c-7c8d-b4ab-629f8ff53e59"
}
```

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@example.com",
    "image": null,
    "role": "USER"
  }
}
```

_(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)_

**หมายเหตุ:**

- `ticket` เป็น opaque one-time token ใช้ได้ครั้งเดียว
- `ticket` หมดอายุภายใน 5 นาที
- frontend callback route ควรอ่าน `ticket` และ `redirectTo` จาก query string แล้วค่อยเรียก endpoint นี้

---

### POST `/api/auth/refresh`

ขอ token ใหม่ด้วย refresh token cookie

- backend ใช้ refresh token rotation แบบเก็บ token family / lineage ไว้เพื่อตรวจ reuse
- ถ้า browser ยิง refresh token เดิมซ้ำพร้อมกันพอดี backend จะคืน successor token เดิมให้ได้ภายในหน้าต่างประมาณ `5 วินาที`
- ถ้า refresh token เก่าถูกใช้ซ้ำนอกหน้าต่างนี้ backend จะถือเป็น suspicious reuse และบังคับ login ใหม่
- ถ้า refresh ล้มเหลวแบบ terminal backend จะส่ง `Set-Cookie` เพื่อล้าง `refresh_token` cookie กลับมาด้วย

**Cookies:** `refresh_token=<token>`

**Response 200:**

```json
{
  "access_token": "eyJhbGciOi...(ใหม่)",
  "user": {
    "id": "uuid-v7",
    "name": "Jane Doe",
    "email": "user@example.com",
    "image": null,
    "role": "USER"
  }
}
```

_(refresh_token ใหม่จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)_

**Frontend Notes:**

- ควรมี refresh request แค่ 1 ตัวที่ in-flight อยู่ในเวลาเดียวกัน
- เมื่อ refresh สำเร็จ ให้ retry original request ได้ `1 ครั้ง`
- ถ้า refresh ตอบ `401` ตามข้อความด้านบน ให้ล้าง auth state แล้วพาไปหน้า login ทันที

---

### POST `/api/auth/logout`

ลบ refresh token (ต้อง login)

**Headers:** `Authorization: Bearer <access_token>`
**Cookies:** `refresh_token=<token>`

**Response 200:**

```json
{
  "message": "Logged out successfully"
}
```

---

## Users

### GET `/api/users/me`

ดู profile ตัวเอง

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:**

```json
{
  "id": "uuid-v7",
  "name": "Jane Doe",
  "email": "user@example.com",
  "image": null,
  "role": "USER",
  "is_active": true,
  "is_verified": true,
  "has_password": true,
  "created_at": "2026-03-25T00:00:00Z"
}
```

---

### PUT `/api/users/me/profile`

อัปเดตชื่อและ/หรือรูปโปรไฟล์ตัวเองผ่าน `multipart/form-data`

**Headers:** `Authorization: Bearer <access_token>`
**Content-Type:** `multipart/form-data`

**Form fields:**

- `name` optional string: ถ้าส่งมา ระบบจะ trim และต้องไม่ว่าง
- `image` optional file: รองรับ `image/jpeg`, `image/png`, `image/webp`, ขนาดไม่เกิน `5 MB`
- `remove_image` optional boolean: ส่ง `true` ถ้าต้องการลบรูปปัจจุบันโดยไม่อัปโหลดรูปใหม่

**Rules:**

- ต้องส่งอย่างน้อยหนึ่งค่าใน `name`, `image`, หรือ `remove_image=true`
- ห้ามส่ง `image` พร้อม `remove_image=true` ใน request เดียวกัน
- ถ้าอัปโหลดรูปใหม่ ระบบจะอัปโหลดไป Cloudinary และลบรูป Cloudinary เก่าของ user ให้หลัง update สำเร็จ
- ถ้ารูปเดิมไม่ได้มาจาก Cloudinary เช่น social avatar เดิม ระบบจะลบแค่ค่าจาก profile แต่จะไม่พยายามลบไฟล์ภายนอก

**Response 200:**

```json
{
  "id": "uuid-v7",
  "name": "Jane Shopper",
  "email": "user@example.com",
  "image": "https://res.cloudinary.com/.../users/abc123.webp",
  "role": "USER",
  "is_active": true,
  "is_verified": true,
  "has_password": true,
  "created_at": "2026-03-25T00:00:00Z"
}
```

**Common errors:**

| Status | Message                                                     | Frontend action                        |
| ------ | ----------------------------------------------------------- | -------------------------------------- |
| `400`  | `Provide at least one of name, image, or remove_image=true` | ตรวจว่ามี field ที่ต้องการ update จริง |
| `400`  | `Name is required.`                                         | แสดง validation error ที่ช่องชื่อ      |
| `400`  | `Unsupported image type...`                                 | บอก user ให้เลือกไฟล์ `jpg/png/webp`   |
| `400`  | `Image is too large...`                                     | บอก user ให้ลดขนาดไฟล์                 |
| `401`  | `Invalid or expired token`                                  | บังคับ login ใหม่หรือ refresh token    |

---

### PUT `/api/users/me/password`

เปลี่ยนรหัสผ่านของบัญชีที่มี password อยู่แล้ว

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "current_password": "old-password-123",
  "new_password": "new-password-123"
}
```

**Response 200:**

```json
{
  "message": "Password changed successfully. Please log in again."
}
```

**หมายเหตุ:**

- `new_password` ต้องมีอย่างน้อย 8 ตัวอักษร และไม่เกิน 128 ตัวอักษร
- ถ้าเรียกจาก browser ควรใช้ `credentials: "include"` เพื่อให้ browser ล้าง `refresh_token` cookie เดิม
- สำเร็จแล้ว backend จะ revoke `refresh_token` ทุกอันของ user และ frontend ควรล้าง access token ฝั่ง client ด้วย

**Common errors:**

| Status | Message                                                                    | Frontend action                                      |
| ------ | -------------------------------------------------------------------------- | ---------------------------------------------------- |
| `400`  | `Password is not set for this account. Use set password instead.`          | พา user ไป flow ตั้งรหัสผ่านครั้งแรก                 |
| `400`  | `New password must be different from the current password`                 | ให้ user ตั้งรหัสใหม่ที่ไม่ซ้ำของเดิม               |
| `401`  | `Current password is incorrect`                                            | แสดง error ที่ช่อง current password                  |
| `401`  | `Invalid or expired token` / `User no longer exists`                       | บังคับ login ใหม่                                    |

---

### POST `/api/users/me/set-password`

ตั้งรหัสผ่านครั้งแรกสำหรับบัญชีที่ยังไม่มี `password` เช่น social-only account

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "new_password": "new-password-123"
}
```

**Response 200:**

```json
{
  "message": "Password set successfully. Please log in again."
}
```

**หมายเหตุ:**

- ถ้าเรียกจาก browser ควรใช้ `credentials: "include"` เพื่อให้ browser ล้าง `refresh_token` cookie เดิม
- สำเร็จแล้ว backend จะ revoke `refresh_token` ทุกอันของ user และ frontend ควรล้าง access token ฝั่ง client ด้วย

**Common errors:**

| Status | Message                                                                    | Frontend action                                      |
| ------ | -------------------------------------------------------------------------- | ---------------------------------------------------- |
| `400`  | `Password is already set for this account. Use change password instead.`   | พา user ไป flow เปลี่ยนรหัสผ่านแทน                  |
| `401`  | `Invalid or expired token` / `User no longer exists`                       | บังคับ login ใหม่                                    |

---

### PUT `/api/users/me`

ขอเปลี่ยน email ใหม่ ระบบจะส่ง verification code ไปที่ email ปลายทางก่อน ยังไม่เปลี่ยนค่าจริงทันที

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "email": "newemail@example.com"
}
```

**Response 200:**

```json
{
  "message": "Verification code sent to your new email address"
}
```

---

### POST `/api/users/me/verify-email-change`

ยืนยัน code เพื่อเปลี่ยน email จริง

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "email": "newemail@example.com",
  "code": "123456"
}
```

**Response 200:**

```json
{
  "id": "uuid-v7",
  "name": "Jane Doe",
  "email": "newemail@example.com",
  "image": null,
  "role": "USER",
  "is_active": true,
  "is_verified": true,
  "has_password": true,
  "created_at": "2026-03-25T00:00:00Z"
}
```

---

## Addresses

### GET `/api/addresses`

ดูที่อยู่ทั้งหมด

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:**

```json
[
  {
    "id": "0195dd2a-9b4c-7c8d-b4ab-629f8ff53e59",
    "recipient_name": "John Doe",
    "phone": "0812345678",
    "address_line": "123/4 ถนนสุขุมวิท",
    "city": "กรุงเทพ",
    "postal_code": "10110",
    "is_default": true,
    "created_at": "2024-03-27T10:30:00Z"
  }
]
```

---

### POST `/api/addresses`

เพิ่มที่อยู่ใหม่

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "recipient_name": "John Doe",
  "phone": "0812345678",
  "address_line": "123/4 ถนนสุขุมวิท",
  "city": "กรุงเทพ",
  "postal_code": "10110",
  "is_default": true
}
```

**Response 201:**

```json
{
  "id": "0195dd2a-9b4c-7c8d-b4ab-629f8ff53e59",
  "recipient_name": "John Doe",
  "phone": "0812345678",
  "address_line": "123/4 ถนนสุขุมวิท",
  "city": "กรุงเทพ",
  "postal_code": "10110",
  "is_default": true,
  "created_at": "2024-03-27T10:30:00Z"
}
```

**หมายเหตุ:** ระบบนี้รองรับเฉพาะที่อยู่ในประเทศไทย ดังนั้น request/response ของ address จะไม่มี field `country`

**กติกา `is_default`:**

- ถ้าไม่ส่ง `is_default` มา ระบบจะถือเป็น `false` ยกเว้น address แรกของ user ซึ่งระบบจะตั้งเป็น default ให้อัตโนมัติ
- 1 user มี default address ได้แค่ 1 ที่อยู่
- ถ้าสร้างหรืออัปเดต address อื่นให้เป็น `is_default=true` ระบบจะ unset default เดิมให้อัตโนมัติ
- ถ้าลบ address ที่เป็น default และยังมี address อื่นเหลืออยู่ ระบบจะ promote address ตัวล่าสุดที่เหลือขึ้นมาเป็น default ให้อัตโนมัติ

---

### PUT `/api/addresses/{id}`

แก้ไขที่อยู่ (ส่งเฉพาะ field ที่ต้องการแก้)

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "recipient_name": "Jane Doe",
  "is_default": true
}
```

**หมายเหตุ:** ถ้าตั้ง address นี้เป็น `is_default=true` ระบบจะ unset default ของ address อื่นใน user เดียวกันให้อัตโนมัติ และถ้าพยายาม unset default ของ address ปัจจุบันในขณะที่ยังมี address อื่นอยู่ ระบบจะรักษาให้ยังมี default อยู่ 1 รายการเสมอ

**Response 200:**

```json
{
  "id": "0195dd2a-9b4c-7c8d-b4ab-629f8ff53e59",
  "recipient_name": "Jane Doe",
  "phone": "0812345678",
  "address_line": "123/4 ถนนสุขุมวิท",
  "city": "กรุงเทพ",
  "postal_code": "10110",
  "is_default": true,
  "created_at": "2024-03-27T10:30:00Z"
}
```

---

### DELETE `/api/addresses/{id}`

ลบที่อยู่

**หมายเหตุ:** ถ้าลบ address ที่เป็น default และยังมี address อื่นเหลืออยู่ ระบบจะเลือก address ตัวล่าสุดที่เหลือขึ้นมาเป็น default ให้อัตโนมัติ

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:**

```json
{
  "message": "Address deleted successfully"
}
```

---

## Products

### GET `/api/products`

ดู products ทั้งหมด (public, ไม่ต้อง login)

**Query Parameters (Optional):**

- `page` (number): หน้าที่ต้องการ เริ่มที่ 1 (default: 1)
- `limit` (number): จำนวนรายการต่อหน้า (default: 10, max: 100)
- `search` (string): ค้นหาจาก `name` และ `description`
- `category_id` (uuid): filter ตาม category
- `min_price` (decimal): ราคาต่ำสุด
- `max_price` (decimal): ราคาสูงสุด
- `in_stock` (boolean): ถ้าเป็น `true` จะคืนเฉพาะสินค้าที่ stock มากกว่า 0
- `sort` (string): `created_desc` (default), `created_asc`, `price_asc`, `price_desc`

**ตัวอย่าง Request:**

```http
GET /api/products?page=1&limit=10&search=iphone&min_price=10000&max_price=50000&in_stock=true&sort=price_asc
```

**หมายเหตุ:**

- ถ้าไม่ส่ง query มาเลย ระบบจะใช้ `page=1` และ `limit=10`
- ถ้า `page < 1` ระบบจะปรับเป็น `1`
- ถ้า `limit > 100` ระบบจะปรับลงเป็น `100`
- ถ้า `min_price > max_price` ระบบจะตอบ `400`
- ถ้า `sort` ไม่อยู่ใน allowed values ระบบจะ fallback เป็น `created_desc`

**Response 200:**

```json
{
  "data": [
    {
      "id": "uuid-v7",
      "name": "iPhone 16",
      "category_id": "category-uuid",
      "category_name": "Smartphones",
      "current_price": "39900",
      "stock": 100,
      "is_active": true
    }
  ],
  "total": 45,
  "page": 1,
  "limit": 20,
  "total_pages": 3
}
```

**Error Example: invalid price range**

```json
{
  "error": {
    "status": 400,
    "message": "min_price must be less than or equal to max_price"
  }
}
```

### GET `/api/products/{id}`

ดู product ตาม id (public)

### GET `/api/products/{id}/images`

ดูรูปทั้งหมดของ product ตามลำดับปัจจุบัน (public)

**Response 200:**

```json
[
  {
    "id": "image-uuid-1",
    "product_id": "product-uuid",
    "image_url": "https://res.cloudinary.com/...",
    "image_public_id": "products/abc123",
    "sort_order": 0,
    "is_primary": true,
    "created_at": "2026-03-25T00:00:00Z"
  }
]
```

### POST `/api/products/upload-image` 🔒 ADMIN

อัปโหลดรูปภาพแบบ Multipart Form-data และรับ URL กลับมา (รูปถูกส่งไปฝากไว้ที่ Cloudinary)

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)
**Content-Type:** `multipart/form-data`

**Body:**

- `image`: [File] (รูปภาพ)

**ข้อจำกัด:**

- รองรับ `image/jpeg`, `image/png`, `image/webp`
- ขนาดไม่เกิน `5 MB`
- รูปที่อัปโหลดแล้วยังไม่ถูกนำไปผูกกับ product จะหมดอายุตาม `PRODUCT_IMAGE_UPLOAD_TTL_MINUTES`

**Response 200:**

```json
{
  "image_url": "https://res.cloudinary.com/...",
  "image_public_id": "products/abc123"
}
```

---

### POST `/api/products` 🔒 ADMIN

สร้าง product

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**หมายเหตุ:** ถ้าจะส่งรูปหลักตอนสร้าง product ต้องใช้ค่าที่ได้จาก `/api/products/upload-image` ของ admin คนเดียวกัน และต้องส่ง `image_url` กับ `image_public_id` มาด้วยกัน

**Request Body:**

```json
{
  "name": "iPhone 16",
  "description": "Latest iPhone",
  "category_id": "category-uuid",
  "image_url": "https://res.cloudinary.com/...",
  "image_public_id": "products/abc123",
  "current_price": 39900.0,
  "stock": 100
}
```

**Error Example: invalid category**

```json
{
  "error": {
    "status": 404,
    "message": "Category not found"
  }
}
```

### PUT `/api/products/{id}` 🔒 ADMIN

แก้ไข product (ส่งเฉพาะ field ที่ต้องการแก้) และใช้สำหรับ replace รูปหลักแบบ backward-compatible

**หมายเหตุ:**

- ถ้าจะส่งรูป ต้องส่ง `image_url` และ `image_public_id` มาด้วยกันเสมอ และรูปใหม่จะถูกใช้ได้เฉพาะถ้าเพิ่งอัปโหลดผ่าน `/api/products/upload-image` ที่ยังไม่หมดอายุ
- ถ้าส่ง `category_id` มา ค่านี้ต้องมีอยู่จริงในระบบ
- ตอนนี้ API นี้ยังไม่รองรับการ clear category ด้วย `null`; ถ้าไม่ส่ง `category_id` ระบบจะคงค่าเดิมไว้

### POST `/api/products/{id}/images` 🔒 ADMIN

เพิ่มรูปเข้า gallery ของ product

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**หมายเหตุ:** ต้องใช้ค่าที่ได้จาก `/api/products/upload-image` ของ admin คนเดียวกันก่อนเสมอ ถ้า `is_primary = true` รูปนี้จะกลายเป็นรูปหลักใหม่ แต่รูปหลักเดิมจะยังอยู่ใน gallery

**Request Body:**

```json
{
  "image_url": "https://res.cloudinary.com/...",
  "image_public_id": "products/def456",
  "is_primary": false
}
```

**Response 200:**

```json
{
  "product": {
    "id": "product-uuid",
    "image_url": "https://res.cloudinary.com/...primary...",
    "image_public_id": "products/abc123"
  },
  "images": [
    {
      "id": "image-uuid-1",
      "product_id": "product-uuid",
      "image_url": "https://res.cloudinary.com/...primary...",
      "image_public_id": "products/abc123",
      "sort_order": 0,
      "is_primary": true,
      "created_at": "2026-03-25T00:00:00Z"
    },
    {
      "id": "image-uuid-2",
      "product_id": "product-uuid",
      "image_url": "https://res.cloudinary.com/...gallery...",
      "image_public_id": "products/def456",
      "sort_order": 1,
      "is_primary": false,
      "created_at": "2026-03-25T00:00:10Z"
    }
  ]
}
```

### PUT `/api/products/{id}/images/reorder` 🔒 ADMIN

จัดลำดับรูปใน gallery ใหม่ โดยรูปแรกใน `image_ids` จะกลายเป็นรูปหลักใหม่

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**Request Body:**

```json
{
  "image_ids": ["image-uuid-2", "image-uuid-1"]
}
```

### DELETE `/api/products/{id}/image` 🔒 ADMIN

ลบรูปหลักของ product

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**หมายเหตุ:** ถ้ายังมีรูปอื่นใน gallery ระบบจะโปรโมตรูปถัดไปขึ้นมาเป็นรูปหลักอัตโนมัติ

### DELETE `/api/products/{id}/images/{image_id}` 🔒 ADMIN

ลบรูปหนึ่งรูปออกจาก gallery

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**หมายเหตุ:** ถ้าลบรูปหลัก ระบบจะโปรโมตรูปถัดไปขึ้นมาเป็นรูปหลักอัตโนมัติ

### DELETE `/api/products/{id}` 🔒 ADMIN

ลบ product (soft delete)

---

## Categories

### GET `/api/categories`

ดู categories ทั้งหมด (public)

**Response 200:**

```json
[
  {
    "id": "category-uuid",
    "name": "Smartphones",
    "description": "Mobile devices",
    "created_at": "2026-03-25T00:00:00Z",
    "updated_at": "2026-03-25T00:00:00Z"
  }
]
```

### POST `/api/categories` 🔒 ADMIN

สร้าง category ใหม่

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**Request Body:**

```json
{
  "name": "Smartphones",
  "description": "Mobile devices"
}
```

**Response 200:**

```json
{
  "id": "category-uuid",
  "name": "Smartphones",
  "description": "Mobile devices",
  "created_at": "2026-03-25T00:00:00Z",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

**Error Example: duplicate category name**

```json
{
  "error": {
    "status": 400,
    "message": "Category already exists"
  }
}
```

---

## Carts

### GET `/api/carts`

ดูตะกร้าสินค้าปัจจุบันของตัวเอง

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:**

```json
{
  "id": "cart-uuid",
  "user_id": "user-uuid",
  "total_amount": "79800.00",
  "items": [
    {
      "id": "cart-item-uuid",
      "product_id": "product-uuid",
      "product_name": "iPhone 16",
      "product_image_url": "https://res.cloudinary.com/...",
      "current_price": "39900.00",
      "stock": 10,
      "is_active": true,
      "quantity": 2,
      "created_at": "2026-03-25T00:00:00Z",
      "updated_at": "2026-03-25T00:00:00Z"
    }
  ],
  "created_at": "2026-03-25T00:00:00Z",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

### POST `/api/carts/items`

เพิ่มสินค้าเข้าตะกร้า หรือเพิ่มจำนวนถ้ามีอยู่แล้ว

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "product_id": "product-uuid",
  "quantity": 1
}
```

**Response 200:** (Updated Cart)

```json
{
  "id": "cart-uuid",
  "user_id": "user-uuid",
  "total_amount": "39900.00",
  "items": [
    {
      "id": "cart-item-uuid",
      "product_id": "product-uuid",
      "product_name": "iPhone 16",
      "product_image_url": "https://res.cloudinary.com/...",
      "current_price": "39900.00",
      "stock": 10,
      "is_active": true,
      "quantity": 1,
      "created_at": "2026-03-25T00:00:00Z",
      "updated_at": "2026-03-25T00:00:00Z"
    }
  ],
  "created_at": "2026-03-25T00:00:00Z",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

**หมายเหตุ:**

- สินค้าต้อง active อยู่
- quantity รวมในตะกร้าต้องไม่เกิน stock ปัจจุบัน

**Error Example: quantity exceeds stock**

```json
{
  "error": {
    "status": 400,
    "message": "Requested quantity exceeds available stock"
  }
}
```

**Error Example: inactive product**

```json
{
  "error": {
    "status": 404,
    "message": "Product not found or inactive"
  }
}
```

### PUT `/api/carts/items/{product_id}`

กำหนดจำนวนสินค้าในตะกร้าแบบ exact quantity

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "quantity": 3
}
```

**หมายเหตุ:**

- ถ้า `quantity = 0` ระบบจะลบสินค้านั้นออกจากตะกร้า
- quantity ต้องไม่เกิน stock ปัจจุบัน
- ถ้า product inactive หรือไม่มีอยู่จริง ระบบจะตอบ `404`
- ถ้า product มี stock ไม่พอ ระบบจะตอบ `400`

**Response 200:** (Updated Cart)

```json
{
  "id": "cart-uuid",
  "user_id": "user-uuid",
  "total_amount": "119700.00",
  "items": [
    {
      "id": "cart-item-uuid",
      "product_id": "product-uuid",
      "product_name": "iPhone 16",
      "product_image_url": "https://res.cloudinary.com/...",
      "current_price": "39900.00",
      "stock": 10,
      "is_active": true,
      "quantity": 3,
      "created_at": "2026-03-25T00:00:00Z",
      "updated_at": "2026-03-25T00:00:00Z"
    }
  ],
  "created_at": "2026-03-25T00:00:00Z",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

### DELETE `/api/carts/items/{product_id}`

ลบสินค้าชิ้นเดียวออกจากตะกร้า

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:** (Updated Cart)

```json
{
  "id": "cart-uuid",
  "user_id": "user-uuid",
  "total_amount": "0.00",
  "items": [],
  "created_at": "2026-03-25T00:00:00Z",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

### DELETE `/api/carts`

ล้างสินค้าทั้งหมดในตะกร้า

**Headers:** `Authorization: Bearer <access_token>`

---

## Orders

### POST `/api/orders`

สร้าง order (snapshot ราคา ณ ตอนสั่ง)

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**

```json
{
  "shipping_address_id": "address-uuid",
  "items": [
    {
      "product_id": "product-uuid",
      "quantity": 2
    }
  ]
}
```

**Response 201:**

```json
{
  "id": "order-uuid",
  "user_id": "user-uuid",
  "shipping_address_id": "address-uuid",
  "total_amount": "79800.00",
  "status": "PENDING",
  "created_at": "2026-03-27T10:30:00Z",
  "items": [
    {
      "id": "order-item-uuid",
      "order_id": "order-uuid",
      "product_id": "product-uuid",
      "product_name_at_purchase": "iPhone 16",
      "quantity": 2,
      "price_at_purchase": "39900.00"
    }
  ]
}
```

---

### GET `/api/orders`

ดูรายการสั่งซื้อของตัวเอง (เรียงจากใหม่ไปเก่า)

**Headers:** `Authorization: Bearer <access_token>`

**Query Parameters:**
- `page` (number): default 1
- `limit` (number): default 20

**Response 200:**

```json
{
  "data": [
    {
      "id": "order-uuid",
      "user_id": "user-uuid",
      "shipping_address_id": "address-uuid",
      "total_amount": "79800.00",
      "status": "PENDING",
      "created_at": "2026-03-27T10:30:00Z"
    }
  ],
  "total": 1,
  "page": 1,
  "limit": 20,
  "total_pages": 1
}
```

---

### GET `/api/orders/{id}`

ดูรายละเอียด order รายการเดียว

**Headers:** `Authorization: Bearer <access_token>`

**Response 200:**

```json
{
  "id": "order-uuid",
  "user_id": "user-uuid",
  "shipping_address_id": "address-uuid",
  "total_amount": "79800.00",
  "status": "PENDING",
  "created_at": "2026-03-27T10:30:00Z",
  "items": [
    {
      "id": "order-item-uuid",
      "order_id": "order-uuid",
      "product_id": "product-uuid",
      "product_name_at_purchase": "iPhone 16",
      "quantity": 2,
      "price_at_purchase": "39900.00"
    }
  ]
}
```
