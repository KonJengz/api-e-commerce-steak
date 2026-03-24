# API Documentation

Base URL: `http://localhost:3000`

---

## Auth

### POST `/api/auth/register`
ส่ง verification code ไปทาง email (ยังไม่สร้าง user)

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
  "message": "Verification code sent to your email"
}
```

---

### POST `/api/auth/verify-email`
ยืนยัน email ด้วย code 6 หลัก → สร้าง user + return tokens

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
  "refresh_token": "a1b2c3d4...",
  "user": {
    "id": "uuid-v7",
    "email": "user@example.com",
    "role": "USER"
  }
}
```

---

### POST `/api/auth/login`
Login ด้วย email + password → return tokens + ส่ง email แจ้งเตือน

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
  "refresh_token": "a1b2c3d4...",
  "user": {
    "id": "uuid-v7",
    "email": "user@example.com",
    "role": "USER"
  }
}
```

---

### POST `/api/auth/google/login`
Login ด้วย Google ID Token → สร้าง user อัตโนมัติ (ถ้ายังไม่มี) + return tokens

**💡 สำคัญเกี่ยวกับ Flow นี้ (ทำไมถึงไม่มี Callback API):** 
ระบบเราออกแบบให้ทำงานแบบ Frontend-Driven OAuth (หรือ Token-based flow) เพื่อลดความซับซ้อน:
1. **Frontend** (Next.js/React) เรียกใช้ `@react-oauth/google` หรือ Firebase เพื่อรับ `id_token` (หรือ `credential`) จาก Google โดยตรงหลังจากผู้ใช้กด Login
2. **Frontend** ส่ง `token` ที่ได้มายิงเข้า API เส้นนี้ (`POST /api/auth/google/login`)
3. **Backend** ตัวนี้จะเช็คความถูกต้องของ Token กับ Server ของ Google และออก Access Token + Refresh Token (Cookie) ให้เหมือนคน Login ปกติ
*(วิธีนี้ ไม่จำเป็นต้องเขียน Callback route บน Backend ให้วุ่นวาย, หน้าบ้านจัดการ Redirect เองได้เลย)*

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
    "email": "user@gmail.com",
    "role": "USER"
  }
}
```
*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

---

### POST `/api/auth/github/login`
Login ด้วย GitHub Access Code → สร้าง user อัตโนมัติ (ถ้ายังไม่มี) + return tokens

**💡 สำคัญเกี่ยวกับ Flow นี้:**
- GitHub ไม่ใช้ระบบ OIDC (ID Token) เหมือน Google แต่ใช้ **Authorization Code Flow**
- หน้าเว็บต้องให้ User ไป login ที่ GitHub ก่อน จากนั้น GitHub จะ redirect กลับมาที่หน้าเว็บพร้อมแนบ `?code=xxxxx` มาใน URL
- หน้าเว็บต้องดึงค่า `code` นี้มายิงเข้า API ต่อไปนี้ เพื่อให้ Backend เอาไปแลกเป็น Profile และออก Token ให้เรา

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
    "email": "user@github.com",
    "role": "USER"
  }
}
```
*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

---

### POST `/api/auth/refresh`
ขอ token ใหม่ด้วย refresh token (rotation: token เก่าจะถูกลบ)

**Request Body:**
```json
{
  "refresh_token": "a1b2c3d4..."
}
```

**Response 200:**
```json
{
  "access_token": "eyJhbGciOi...(ใหม่)",
  "refresh_token": "x9y8z7w6...(ใหม่)",
  "user": {
    "id": "uuid-v7",
    "email": "user@example.com",
    "role": "USER"
  }
}
```

---

### POST `/api/auth/logout`
ลบ refresh token (ต้อง login)

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**
```json
{
  "refresh_token": "a1b2c3d4..."
}
```

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
  "email": "user@example.com",
  "role": "USER",
  "is_active": true,
  "is_verified": true,
  "created_at": "2026-03-25T00:00:00Z"
}
```

---

### PUT `/api/users/me`
แก้ไข profile

**Headers:** `Authorization: Bearer <access_token>`

**Request Body:**
```json
{
  "email": "newemail@example.com"
}
```

---

## Addresses

### GET `/api/addresses`
ดูที่อยู่ทั้งหมด

**Headers:** `Authorization: Bearer <access_token>`

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
  "country": "Thailand",
  "is_default": true
}
```

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

---

### DELETE `/api/addresses/{id}`
ลบที่อยู่

**Headers:** `Authorization: Bearer <access_token>`

---

## Products

### GET `/api/products`
ดู products ทั้งหมด (public, ไม่ต้อง login)

### GET `/api/products/{id}`
ดู product ตาม id (public)

### POST `/api/products` 🔒 ADMIN
สร้าง product

**Headers:** `Authorization: Bearer <access_token>` (ADMIN only)

**Request Body:**
```json
{
  "name": "iPhone 16",
  "description": "Latest iPhone",
  "image_url": "https://example.com/iphone.jpg",
  "current_price": 39900.00,
  "stock": 100
}
```

### PUT `/api/products/{id}` 🔒 ADMIN
แก้ไข product (ส่งเฉพาะ field ที่ต้องการแก้)

### DELETE `/api/products/{id}` 🔒 ADMIN
ลบ product (soft delete)

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
    { "product_id": "product-uuid-1", "quantity": 2 },
    { "product_id": "product-uuid-2", "quantity": 1 }
  ]
}
```

**Response 200:**
```json
{
  "id": "order-uuid",
  "user_id": "user-uuid",
  "shipping_address_id": "address-uuid",
  "total_amount": 119700.00,
  "status": "PENDING",
  "created_at": "2026-03-25T00:00:00Z",
  "items": [
    {
      "id": "item-uuid",
      "order_id": "order-uuid",
      "product_id": "product-uuid-1",
      "product_name_at_purchase": "iPhone 16",
      "quantity": 2,
      "price_at_purchase": 39900.00
    }
  ]
}
```

### GET `/api/orders`
ดู orders ของตัวเอง

**Headers:** `Authorization: Bearer <access_token>`

### GET `/api/orders/{id}`
ดูรายละเอียด order พร้อม items

**Headers:** `Authorization: Bearer <access_token>`
