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
- ถ้าเรียกจาก browser frontend, origin ของ frontend ต้องตรงกับค่า `APP_URL` ที่ backend ใช้อยู่
- ถ้า deploy หลาย environment ควรแยก base URL และ frontend origin ให้ตรงกันในแต่ละ environment

---

## Auth

### POST `/api/auth/register`
สร้าง user แบบยังไม่ยืนยันอีเมล (`is_verified = false`) และส่ง verification code ไปทาง email

**Request Body:**
```json
{
  "name": "Jane Doe",
  "email": "user@example.com",
  "password": "12345678",
  "image": "https://cdn.example.com/avatar.png"
}
```

`image` เป็น optional ส่งมาก็ได้ ไม่ส่งก็ได้

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

*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

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

**ถ้ายังไม่ verify email:** backend จะตอบ `403` พร้อมข้อความ `Email not verified`

---

### GET `/api/auth/google/start`
เริ่ม Google OAuth จาก backend โดยส่ง browser ไปยัง Google

**Query Params:**
- `exchange_url`: URL callback ของ frontend ที่ backend จะ redirect กลับหลัง login สำเร็จ
- `redirect_to`: path ปลายทางหลัง frontend exchange ticket เสร็จ เช่น `/account`

**Flow:**
1. Frontend พา browser มายิง endpoint นี้
2. Backend สร้าง state cookie และ redirect ไป Google
3. Google redirect กลับ `GET /api/auth/google/callback`
4. Backend สร้าง one-time login ticket แล้ว redirect กลับ `exchange_url?ticket=...`

---

### GET `/api/auth/google/callback`
Google redirect callback ของ backend

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

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
*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

---

### GET `/api/auth/github/start`
เริ่ม GitHub OAuth จาก backend โดยส่ง browser ไปยัง GitHub

**Query Params:**
- `exchange_url`: URL callback ของ frontend ที่ backend จะ redirect กลับหลัง login สำเร็จ
- `redirect_to`: path ปลายทางหลัง frontend exchange ticket เสร็จ เช่น `/account`

---

### GET `/api/auth/github/callback`
GitHub redirect callback ของ backend

**หมายเหตุ:** route นี้เอาไว้ให้ provider เรียกกลับ ไม่ได้ออกแบบให้ frontend เรียกตรง

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
*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

---

### POST `/api/auth/oauth/exchange`
แลก one-time social login ticket ที่ backend ออกให้ใน callback กลับมาเป็น access token + refresh token

ใช้โดย frontend callback route หลังจาก backend social OAuth สำเร็จแล้ว

**Request Body:**
```json
{
  "ticket": "0195c0f0-7d62-7a9c-9d3e-4f2d13bba1df"
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
*(refresh_token จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

---

### POST `/api/auth/refresh`
ขอ token ใหม่ด้วย refresh token cookie (rotation: token เก่าจะถูกลบแบบ single-use)

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
*(refresh_token ใหม่จะถูกส่งผ่าน `Set-Cookie` header แบบ HttpOnly)*

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
  "created_at": "2026-03-25T00:00:00Z"
}
```

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
  "created_at": "2026-03-25T00:00:00Z"
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
  "current_price": 39900.00,
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
  "image_ids": [
    "image-uuid-2",
    "image-uuid-1"
  ]
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

### DELETE `/api/carts/items/{product_id}`
ลบสินค้าชิ้นเดียวออกจากตะกร้า

**Headers:** `Authorization: Bearer <access_token>`

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

**Query Parameters (Optional):**
- `page` (number): หน้าที่ต้องการ เริ่มที่ 1 (default: 1)
- `limit` (number): จำนวนรายการต่อหน้า (default: 20, max: 100)

**ตัวอย่าง Request:**
```http
GET /api/orders?page=1&limit=20
```

**หมายเหตุ:**
- ถ้าไม่ส่ง query มาเลย ระบบจะใช้ `page=1` และ `limit=20`
- ถ้า `page < 1` ระบบจะปรับเป็น `1`
- ถ้า `limit > 100` ระบบจะปรับลงเป็น `100`

**Response 200:**
```json
{
  "data": [
    {
      "id": "order-uuid",
      "total_amount": "119700.00",
      "status": "PENDING"
    }
  ],
  "total": 15,
  "page": 1,
  "limit": 20,
  "total_pages": 1
}
```

### GET `/api/orders/{id}`
ดูรายละเอียด order พร้อม items

**Headers:** `Authorization: Bearer <access_token>`
